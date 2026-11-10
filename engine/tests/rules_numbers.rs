//! Unbounded numbers (M6, ENGINE.md 46; F7, 100.7): Magic has no maximum for
//! counters, life, power, toughness or damage (CR 107.1, 122.1, 119, 120,
//! 208). The engine keeps every such value exact: stored counts and amounts
//! are 64-bit, derived values (power, toughness, sums) `i128`, and a number
//! the game would keep past `i64::MAX` ends the game **aborted** (no result)
//! instead of saturating, wrapping or truncating. Every feature `encode`
//! writes stays finite and bounded (|x| <= `encode::FEATURE_BOUND`, 256)
//! whatever the game state.
//!
//! The reproduction of the mtg5 incident (iteration 14, 2026-10-27): an
//! Ouroboroid board grew past 65,535 counters a creature (the old `u16`
//! counts saturated there) and past 1.31 million total power, whose global
//! feature (total power / 20) was +inf in the float16 self-play shard.

mod common;

use common::db;
use engine::az::encode::NUM_FEATURES;
use engine::db::Counter;
use engine::db::AbilitySlot;
use engine::game::EndReason;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

/// The largest finite float16 (the self-play shards store the features as
/// float16: anything larger becomes +inf).
const F16_MAX: f32 = 65504.0;

/// The bound every encoded feature keeps.
const BOUND: f32 = engine::az::encode::FEATURE_BOUND;

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

/// Both players' encodings of `g`: every value finite, within `BOUND`, and
/// so (a fortiori) finite as float16.
fn assert_features_bounded(g: &Game, what: &str) {
    for viewer in 0..2u8 {
        let mut x = vec![0f32; NUM_FEATURES];
        g.encode_features(viewer, &mut x);
        for (i, &v) in x.iter().enumerate() {
            assert!(
                v.is_finite() && v.abs() <= BOUND,
                "{what}: feature {i} for P{viewer} is {v} (bound {BOUND}; {} as float16)",
                if v.abs() > F16_MAX { "+inf" } else { "finite" }
            );
        }
    }
}

/// The mtg5 board: Ouroboroid's beginning-of-combat trigger (X = its power)
/// on a creature that already has 65,000 +1/+1 counters, and twenty more
/// creatures. Every count is exact past 65,535 (CR 122.1: no maximum), the
/// total power passes 1.31 million, and the features stay bounded.
#[test]
fn ouroboroid_counters_past_65535_stay_exact_and_the_features_bounded_cr_122_1() {
    let mut g = scen()
        .battlefield(0, &["Ouroboroid"])
        .battlefield(0, &["Test Bear"; 20])
        .battlefield(1, &["Test Reacher"])
        .counters(0, "Ouroboroid", Counter::P1P1, 65_000)
        .at(Step::Main1, 0)
        .start();
    let o = bf(&g, 0, "Ouroboroid");
    assert_eq!(g.pt(o), (65_001, 65_003));
    quiet_until(&mut g, |g| g.turn().step == Step::BeginCombat && !g.stack().is_empty());
    settle(&mut g);
    // X = 65,001: Ouroboroid 65,000 + 65,001, each bear 65,001.
    assert_eq!(g.counters_on(o, Counter::P1P1) as u64, 130_001, "Ouroboroid's counters (saturated or wrapped at u16?)");
    assert_eq!(g.pt(o), (130_002, 130_004));
    let bears: Vec<CardIdx> =
        g.battlefield().iter().copied().filter(|&c| g.obj(c).controller == 0 && g.name(c) == "Test Bear").collect();
    assert_eq!(bears.len(), 20);
    for &b in &bears {
        assert_eq!(g.counters_on(b, Counter::P1P1) as u64, 65_001);
        assert_eq!(g.pt(b), (65_003, 65_003));
    }
    let total: i64 = g.battlefield().iter().filter(|&&c| g.obj(c).controller == 0).map(|&c| g.power(c) as i64).sum();
    assert_eq!(total, 130_002 + 20 * 65_003);
    assert!(total > 1_310_000);
    assert_features_bounded(&g, "after Ouroboroid's trigger");
    // Next turn X = 130,002: past 196,000 counters on each bear.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.turn().step == Step::BeginCombat && !g.stack().is_empty());
    settle(&mut g);
    assert_eq!(g.counters_on(o, Counter::P1P1) as u64, 260_003);
    for &b in &bears {
        assert_eq!(g.counters_on(b, Counter::P1P1) as u64, 65_001 + 130_002);
    }
    assert_features_bounded(&g, "a turn later");
}

/// Combat damage past 65,535 is dealt in full (CR 510.2, 120.3a): a 65,537
/// power creature unblocked takes a 65,537-life player to 0.
#[test]
fn combat_damage_past_65535_is_dealt_in_full_cr_510_2() {
    let mut g = scen()
        .battlefield(0, &["Test Bear"])
        .counters(0, "Test Bear", Counter::P1P1, 65_535)
        .life(1, 65_537)
        .at(Step::DeclareAttackers, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.power(bear), 65_537);
    assert_eq!(g.kind(), Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    for _ in 0..200 {
        if g.is_over() {
            break;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareBlockers | DecisionKind::DeclareAttackers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
        if g.turn().step == Step::EndCombat {
            break;
        }
    }
    assert_eq!(g.life(1), 0, "dealt 65,537 combat damage");
    assert!(g.is_over() && g.outcome().unwrap().winner == Some(0));
}

/// Lethal damage for a trampler's assignment is the blocker's whole
/// toughness (CR 702.19b, 510.1c-d), not that toughness truncated to 16
/// bits: a 65,539/65,539 trampler blocked by a 65,535/65,539 wall must
/// assign all of its damage to the wall (nothing tramples over).
#[test]
fn lethal_damage_past_65535_for_a_tramplers_assignment_cr_702_19b() {
    let mut g = scen()
        .battlefield(0, &["Test Trampler"])
        .battlefield(1, &["Test Wall"])
        .counters(0, "Test Trampler", Counter::P1P1, 65_535)
        .counters(1, "Test Wall", Counter::P1P1, 65_535)
        .at(Step::DeclareAttackers, 0)
        .start();
    let t = bf(&g, 0, "Test Trampler");
    let w = bf(&g, 1, "Test Wall");
    assert_eq!(g.pt(t), (65_539, 65_539));
    assert_eq!(g.pt(w), (65_535, 65_539));
    g.choose(Choice::Card(t));
    g.choose(Choice::Pass);
    let mut asked = None;
    for _ in 0..200 {
        let d = g.pending_decision().expect("game running");
        match d.kind {
            DecisionKind::DeclareBlockers => {
                // (The wall is asked which attacker it blocks.)
                if g.pending_decision().unwrap().options.iter().any(|o| o.choice == Choice::Card(t)) {
                    g.choose(Choice::Card(t));
                } else {
                    g.choose(Choice::Pass);
                }
            }
            DecisionKind::AssignDamage => {
                asked = Some(g.options_desc());
                // Any legal assignment: the wall must get lethal damage first.
                let c = d.options.last().unwrap().choice;
                g.choose(c);
            }
            DecisionKind::Priority | DecisionKind::DeclareAttackers => {
                g.choose(Choice::Pass);
            }
            _ => {
                let c = d.options[0].choice;
                g.choose(c);
            }
        }
        if g.turn().step == Step::EndCombat {
            break;
        }
    }
    assert!(g.blockers_of(t).is_empty() || g.turn().step == Step::EndCombat);
    if let Some(opts) = &asked {
        assert!(opts.len() == 1, "only one assignment is legal (all 65,539 to the wall): {opts:?}");
    }
    assert_eq!(g.life(1), 20, "nothing trampled over (the wall's lethal damage is 65,539)");
    assert!(g.find_card(1, "Test Wall", Zone::Battlefield).is_none(), "the wall was dealt lethal damage");
    assert!(g.find_card(0, "Test Trampler", Zone::Battlefield).is_some(), "the trampler survives 65,535 damage");
}

// ---- F7 (ENGINE.md 100.7): exact past 32 bits, aborted past 64 -------------------

/// Pay with the first source offered until the payment is done.
fn pay(g: &mut Game) {
    let paying = |g: &Game| matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana));
    g.drive_until(|g| at_priority(g) || !paying(g));
    while paying(g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !paying(g));
    }
}

/// Attack with `attacker` (nothing blocks) and play on to the end of combat
/// or the end of the game.
fn attack_unblocked(g: &mut Game, attacker: CardIdx) {
    assert_eq!(g.kind(), Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(attacker));
    g.choose(Choice::Pass);
    for _ in 0..200 {
        if g.is_over() || g.turn().step == Step::EndCombat {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareBlockers | DecisionKind::DeclareAttackers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("attack_unblocked: no progress\n{}", g.log_lines().join("\n"));
}

/// The game ended aborted: no winner, no result (ENGINE.md 100.7).
fn assert_aborted(g: &Game, what: &str) {
    assert!(g.is_over(), "{what}: the game goes on\n{}", g.log_lines().join("\n"));
    assert!(g.aborted(), "{what}: {:?}", g.outcome());
    let o = g.outcome().unwrap();
    assert_eq!(o.reason, EndReason::Aborted, "{what}");
    assert_eq!(o.winner, None, "{what}");
    assert!(g.pending_decision().is_none(), "{what}: no decision after an abort");
}

/// Bristly Bill doubles the +1/+1 counters of a creature with 3,000,000,000
/// of them: 6,000,000,000, past `u32::MAX` and exact (CR 122.1: no
/// maximum; the old `u32` counts saturated at 4,294,967,295), and its power
/// and toughness with them (CR 613.4c).
#[test]
fn cr_122_1_counters_doubled_past_u32_max_stay_exact() {
    let mut g = scen()
        .battlefield(0, &["Bristly Bill, Spine Sower", "Test Bear"])
        .battlefield(0, &["Forest"; 5])
        .battlefield(1, &["Test Reacher"])
        .counters(0, "Test Bear", Counter::P1P1, 3_000_000_000)
        .at(Step::Main1, 0)
        .start();
    let bill = bf(&g, 0, "Bristly Bill, Spine Sower");
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(bear), (3_000_000_002, 3_000_000_002));
    g.choose(Choice::Ability(bill, AbilitySlot::Activated(0)));
    pay(&mut g);
    settle(&mut g);
    assert!(!g.is_over());
    assert_eq!(g.counters_on(bear, Counter::P1P1), 6_000_000_000, "doubled exactly");
    assert_eq!(g.pt(bear), (6_000_000_002, 6_000_000_002));
    assert_features_bounded(&g, "6,000,000,000 counters");
    // And again: 12,000,000,000.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(Choice::Ability(bill, AbilitySlot::Activated(0)));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 12_000_000_000);
    assert_eq!(g.power(bear), 12_000_000_002);
}

/// Combat damage and a life total past `i32::MAX` are exact (CR 510.2,
/// 120.3a, 119.3): a creature with 3,000,000,000 +1/+1 counters deals
/// 3,000,000,002 to a player at 5,000,000,000 life, who is left at
/// 1,999,999,998 (the old `i32` amounts saturated at 2,147,483,647).
#[test]
fn cr_510_2_combat_damage_and_life_past_i32_max_are_exact() {
    let mut g = scen()
        .battlefield(0, &["Test Lifelinker"])
        .counters(0, "Test Lifelinker", Counter::P1P1, 3_000_000_000)
        .life(0, 4_000_000_000)
        .life(1, 5_000_000_000)
        .at(Step::DeclareAttackers, 0)
        .start();
    let l = bf(&g, 0, "Test Lifelinker");
    assert_eq!(g.power(l), 3_000_000_002);
    attack_unblocked(&mut g, l);
    assert!(!g.is_over(), "{:?}", g.outcome());
    assert_eq!(g.life(1), 1_999_999_998, "5,000,000,000 - 3,000,000,002");
    assert_eq!(g.life(0), 7_000_000_002, "lifelink: 4,000,000,000 + 3,000,000,002 (CR 702.15b)");
    assert_eq!(g.player(0).life_gained, 3_000_000_002, "the life gained this turn");
    assert_features_bounded(&g, "life past i32");
}

/// A life total pushed past `i64::MAX` (CR 119.3 has no maximum) ends the
/// game aborted: lifelink's gain from a player at `i64::MAX - 1` life
/// (ENGINE.md 100.7: a number the game keeps leaves its exact range).
#[test]
fn cr_119_3_a_life_total_past_i64_ends_the_game_aborted() {
    let mut g = scen()
        .battlefield(0, &["Test Lifelinker"])
        .life(0, i64::MAX - 1)
        .at(Step::DeclareAttackers, 0)
        .start();
    let l = bf(&g, 0, "Test Lifelinker");
    attack_unblocked(&mut g, l);
    assert_aborted(&g, "life past i64::MAX");
    assert_eq!(g.life(1), 18, "the damage was dealt before the gain");
    assert_features_bounded(&g, "an aborted game at i64::MAX life");
}

/// A count of counters pushed past `i64::MAX` (CR 122.1 has no maximum)
/// ends the game aborted: Bristly Bill doubling 5,000,000,000,000,000,000
/// +1/+1 counters. The board before the doubling (power past 2^62)
/// encodes finite and bounded.
#[test]
fn cr_122_1_a_counter_count_past_i64_ends_the_game_aborted() {
    let mut g = scen()
        .battlefield(0, &["Bristly Bill, Spine Sower", "Test Bear"])
        .battlefield(0, &["Forest"; 5])
        .counters(0, "Test Bear", Counter::P1P1, 5_000_000_000_000_000_000)
        .at(Step::Main1, 0)
        .start();
    let bill = bf(&g, 0, "Bristly Bill, Spine Sower");
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.power(bear), 5_000_000_000_000_000_002);
    assert_features_bounded(&g, "5 x 10^18 counters");
    g.choose(Choice::Ability(bill, AbilitySlot::Activated(0)));
    pay(&mut g);
    for _ in 0..50 {
        if g.is_over() {
            break;
        }
        let c = match g.kind() {
            Some(DecisionKind::Priority) => Choice::Pass,
            _ => g.pending_decision().unwrap().options[0].choice,
        };
        g.choose(c);
    }
    assert_aborted(&g, "counters past i64::MAX");
    assert_features_bounded(&g, "an aborted game past i64 counters");
}

/// Power past `i64::MAX` is exact as a derived value (layer 7 in `i128`, CR
/// 613.4c: a 2/2 with `i64::MAX` +1/+1 counters has power 2^63 + 1), and
/// assigning it as combat damage, a number the game keeps, ends the game
/// aborted (CR 510.1a; ENGINE.md 100.7).
#[test]
fn cr_510_1a_combat_damage_past_i64_ends_the_game_aborted() {
    let mut g = scen()
        .battlefield(0, &["Test Bear"])
        .counters(0, "Test Bear", Counter::P1P1, i64::MAX as u64)
        .at(Step::DeclareAttackers, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.power(bear), i64::MAX as i128 + 2);
    assert_features_bounded(&g, "power past i64");
    attack_unblocked(&mut g, bear);
    assert_aborted(&g, "combat damage past i64::MAX");
}
