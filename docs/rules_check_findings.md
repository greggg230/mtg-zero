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
