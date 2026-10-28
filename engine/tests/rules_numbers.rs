//! Unbounded numbers (M6, ENGINE.md 46): Magic has no maximum for counters,
//! life, power, toughness or damage (CR 107.1, 122.1, 119, 120, 208). The
//! engine keeps every such value exact up to its class-B bound
//! (docs/shortcuts.md 46.1) and saturates there, never wrapping or
//! truncating; and every feature `encode` writes stays finite and bounded
//! (|x| <= `encode::FEATURE_BOUND`, 256) whatever the game state.
//!
//! The reproduction of the mtg5 incident (iteration 14, 2026-10-27): an
//! Ouroboroid board grew past 65,535 counters a creature (the old `u16`
//! counts saturated there) and past 1.31 million total power, whose global
//! feature (total power / 20) was +inf in the float16 self-play shard.

mod common;

use common::db;
use engine::az::encode::NUM_FEATURES;
use engine::db::Counter;
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
