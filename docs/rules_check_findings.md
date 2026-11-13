# Rules-check findings

Engine rules bugs found by the rule-derived invariants (`engine/src/rules_check.rs`, `docs/ENGINE.md` 131)
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

## STATUS (2026-11-12, session handoff; branch `fuzz-invariants`, from main 267ae603; nothing pushed or merged)

**Done.**
* `engine/src/rules_check.rs` (`pub mod rules_check`, never called by the engine: zero cost in release):
  `RulesCheck` (per game: `new(&g)`, `step(&g)` after every apply; stops after its first violation),
  `MatchCheck` (best-of-three games), `check_state` (one state), `DraftCheck` (team draft events), `Stats`.
  Invariants: card conservation 108.1/108.3/400.11b/100.4/702.139; tokens and copies 704.5d-e, 111.8, 707.10a;
  at priority 117.5 and 704.5a-c, f, g, h (incl. the record of deathtouch damage), i, j, k, m, n, p, q, s, t, z
  (704.5v-y battles fail loudly: unmodelled); 502.4, 104.3; the stack 405.2, 608.1/405.5, 700.2, 107.3a, 405.4,
  115.7; turns 500.1, 102.1, 500.7, 500.8, 103.8; combat options 508.1a, 509.1a-b, 510.1c/702.19b/702.2c;
  declarations as made 508.1a/b/d/f, 302.6, 702.3b, 702.20b, 509.1a/b/h, 702.9b, 702.17b, 702.28b, 702.14c,
  702.16f, 702.111b; mana 500.5; life 119.7; draft 100.2b, 100.4b. Comparing checks apply only when no spell or
  ability resolved between the two states or was resolving at either.
* 28 unit tests (crafted violations caught, legal look-alikes pass) + 5 ignored repro tests (F1-F5).
* Wiring: `engine/tests/fuzz.rs` (games, determinized and weighted worlds, chance walks, matches and their
  worlds, hidden decklists and deck-choice worlds; record-and-continue, `rules_verdict` at each test's end,
  `FUZZ_RULES_ALLOW=<rules>`), `engine/tests/draft.rs` (every event step, worlds via `finish_random`,
  heuristic-played matches through new `draft_bots::play_match_heuristic_with`).
* Digests (desktop, this branch vs main built side by side): full 438164c0429d7fa3, `DIGEST_ENCODING=0`
  a0062bb5569e06d3: both equal main's.
* Throughput (default pool, 1,500 games, same seeds side by side): main 172.3 s, this branch 176.2 s (+2.3%);
  200 matches 59.7 s vs 56.5 s (noise). Same game outcomes.

**Fuzz runs so far (desktop, single thread, seeds from 0; violations listed are real engine bugs only).**
default 2,000 games (F1 at seed 366 only); interactions 2,000 (clean after fixes); default matches 300 (clean);
delver 1,000 (clean); eoe 1,000 (clean); doomsday, dd_a, dd_b, dd_c, sneak_show, sns, delver_sb 300 each
(clean); names 300, sealed 200, `FUZZ_HIDDEN_DECKS=1` 200 (clean); draft events 100 (clean). Exponential:
not finished. Not yet run at scale: everything (the 20k campaign).

**Open issue (blocks `FUZZ_CHANCE`).** Chance walks report false positives: CR 108.1 (a forced chance outcome
swaps an unknown library card's identity, `force_library_card`, then the card is drawn: the exemption must
compare each slot with the previous state, skipping slots that were unknown library cards there, and the walk
must tell the check it forced: add `RulesCheck::forced_chance()` called from `chance_step`), and CR 500.7
(the checker's normal-turn record goes wrong in walks; debugging showed worlds created at a Time Vault
"skip that turn" decision, `may_skip`: check `turns()`'s re-sync after a skipped check). Fix both before the
campaign, rerun `FUZZ_CHANCE=20 FUZZ_GAMES=200`.

**Left, in order.**
1. Fix the open issue above; commit.
2. The 20k campaign (Mac: `bash tools/mac.sh` with `CARGO_BUILD_JOBS=4` and at most 4 processes; desktop at
   most ~6): `FUZZ_GAMES=20000` per pool (default, interactions, exponential, eoe, sealed, delver, delver_sb,
   doomsday, dd_a, dd_b, dd_c, names, sneak_show, sns, `FUZZ_HIDDEN_DECKS=1`) split in `FUZZ_FIRST` chunks,
   `--exact fuzz_random_games`; `FUZZ_MATCHES=2000 --exact fuzz_random_matches` for default, interactions,
   drafted, delver (+ its post-sideboard matches); `DRAFT_FUZZ=20000` (draft test). Triage each violation:
   invariant bug -> fix exactly; engine bug -> add here with a repro (don't fix the engine on this branch).
   Expect F1 hits (704.5h) in pools with deathtouch and indestructible.
3. Gates (engine tier): `python tools/gate.py engine --tested-sha <head> --expect-digest 438164c0429d7fa3
   --expect-digest0 a0062bb5569e06d3`, `cargo test --release -p engine --test az`, the full engine suite, pytest
   (after `bash tools/build_py.sh`).
4. ENGINE.md section 131 (next free number; main ends at 130): the module, the invariant table with CR refs,
   the ones left out (608.2b target legality: the engine's own targeting predicates; 601.2f-h mana paid =
   cost and 119/120 life accounting: need payment/damage instrumentation that would check the engine's single
   writer against itself; 510.4 who deals damage in which step: needs a hook; 704.5r/u/aa: no such abilities;
   704.5v-y: no battles), the campaign's totals per pool, throughput, findings.
5. Not now (the maintainer's): merge into main; the farm branch port: copy the fuzz.rs hunks into
   `engine/src/fuzz/games.rs` / `draft.rs` with `crate::rules_check` (one `RulesCheck::new` at game start and
   one `step` after each apply per loop; there a violation should fail the seed like a panic, the farm's
   model), and `play_match_heuristic_with` comes with this branch.
