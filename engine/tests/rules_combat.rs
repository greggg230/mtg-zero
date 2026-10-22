//! Combat (CR 506-511) and the combat keywords (CR 702).

mod common;

use common::db;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};

const LIB: &[&str] = &["Island"; 5];

fn scen() -> Scenario {
    Scenario::new(db()).library(0, LIB).library(1, LIB)
}

fn at_attack(p0: &[&str], p1: &[&str]) -> Game {
    scen().battlefield(0, p0).battlefield(1, p1).at(Step::DeclareAttackers, 0).start()
}

fn attack_options(g: &Game) -> Vec<CardIdx> {
    assert_eq!(g.kind(), Some(DecisionKind::DeclareAttackers));
    g.pending_decision()
        .unwrap()
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Card(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn finish_combat(g: &mut Game) {
    g.pass_until(|g| g.turn().step >= Step::EndCombat);
}

#[test]
fn cr_302_6_summoning_sickness_and_cr_702_10_haste() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Mountain", "Mountain"])
        .hand(0, &["Test Bear", "Test Hasty"])
        .at(Step::Main1, 0)
        .start();
    for name in ["Test Bear", "Test Hasty"] {
        let c = g.card(0, name, Zone::Hand);
        g.choose(Choice::Cast(c));
        g.pass_until(|g| g.stack().is_empty());
    }
    g.pass_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let opts = attack_options(&g);
    let names: Vec<&str> = opts.iter().map(|&c| g.name(c)).collect();
    assert_eq!(names, vec!["Test Hasty"]);
}

#[test]
fn cr_508_1f_vigilance_and_cr_702_3b_defender() {
    let mut g = at_attack(&["Test Bear", "Test Vigilant", "Test Wall"], &[]);
    let bear = g.card(0, "Test Bear", Zone::Battlefield);
    let vig = g.card(0, "Test Vigilant", Zone::Battlefield);
    assert_eq!(attack_options(&g), vec![bear, vig], "defender can't attack");
    g.choose(Choice::Card(bear));
    // Attackers are added in increasing slot order: the Bear is no longer offered.
    assert_eq!(attack_options(&g), vec![vig]);
    g.choose(Choice::Card(vig));
    assert!(g.obj(bear).tapped);
    assert!(!g.obj(vig).tapped);
    finish_combat(&mut g);
    assert_eq!(g.life(1), 16);
}

#[test]
fn cr_509_1b_flying_needs_flying_or_reach_to_block() {
    let mut g = at_attack(&["Test Flyer"], &["Test Bear", "Test Reacher"]);
    let flyer = g.card(0, "Test Flyer", Zone::Battlefield);
    g.choose(Choice::Card(flyer));
    // The Bear's only option (no block) was answered; the Reacher decides.
    assert_eq!(g.kind(), Some(DecisionKind::DeclareBlockers));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2);
    let reacher = g.card(1, "Test Reacher", Zone::Battlefield);
    g.choose(Choice::Card(flyer));
    finish_combat(&mut g);
    assert_eq!(g.life(1), 20, "the flyer was blocked");
    assert_eq!(g.obj(reacher).zone, Zone::Battlefield);
}

#[test]
fn cr_702_110b_menace_needs_two_blockers() {
    // One potential blocker: it may not block alone.
    let mut g = at_attack(&["Test Menace"], &["Test Bear"]);
    let m = g.card(0, "Test Menace", Zone::Battlefield);
    g.choose(Choice::Card(m));
    finish_combat(&mut g);
    assert_eq!(g.life(1), 17);

    // Two: if the first blocks, the second must block too.
    let mut g = at_attack(&["Test Menace"], &["Test Bear", "Test Bear"]);
    let m = g.card(0, "Test Menace", Zone::Battlefield);
    g.choose(Choice::Card(m));
    assert_eq!(g.kind(), Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(m));
    // The second Bear had only one option (block) and it was taken.
    assert_eq!(g.kind(), Some(DecisionKind::AssignDamage));
    let blockers: Vec<CardIdx> = g.blockers_of(m);
    assert_eq!(blockers.len(), 2);
}

#[test]
fn cr_510_1c_attacking_player_divides_damage_among_blockers() {
    let mut g = at_attack(&["Cogwork Librarian"], &["Test Bear", "Test Bear"]);
    let cl = g.card(0, "Cogwork Librarian", Zone::Battlefield);
    g.choose(Choice::Card(cl));
    g.choose(Choice::Card(cl));
    g.choose(Choice::Card(cl));
    assert_eq!(g.kind(), Some(DecisionKind::AssignDamage));
    assert_eq!(g.to_move(), Some(0));
    // First blocker: any of 0..=3; the last gets the rest.
    assert_eq!(g.pending_decision().unwrap().options.len(), 4);
    g.choose(Choice::Number(3));
    finish_combat(&mut g);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1);
    assert_eq!(g.obj(cl).zone, Zone::Graveyard, "took 4 damage");
}

#[test]
fn cr_702_19c_trample_assigns_excess_only_after_lethal() {
    // Deathtouch + trample: 1 is lethal to each blocker (702.2c).
    let mut g = at_attack(&["Test Giant"], &["Test Bear", "Test Bear"]);
    let giant = g.card(0, "Test Giant", Zone::Battlefield);
    g.choose(Choice::Card(giant));
    g.choose(Choice::Card(giant));
    g.choose(Choice::Card(giant));
    g.choose(Choice::Number(1));
    // Last blocker: lethal (1) up to all that is left (4).
    let n: Vec<String> = g.options_desc();
    assert_eq!(n.len(), 4, "{n:?}");
    g.choose(Choice::Number(1));
    finish_combat(&mut g);
    assert_eq!(g.life(1), 17);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 2);

    // Less than lethal to an earlier blocker: nothing tramples over.
    let mut g = at_attack(&["Test Trampler"], &["Test Bear", "Test Bear"]);
    let tr = g.card(0, "Test Trampler", Zone::Battlefield);
    g.choose(Choice::Card(tr));
    g.choose(Choice::Card(tr));
    g.choose(Choice::Card(tr));
    g.choose(Choice::Number(1));
    finish_combat(&mut g);
    assert_eq!(g.life(1), 20);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
}

#[test]
fn cr_702_4_first_strike_deals_damage_first() {
    let mut g = at_attack(&["Test Striker"], &["Test Bear"]);
    let s = g.card(0, "Test Striker", Zone::Battlefield);
    g.choose(Choice::Card(s));
    g.choose(Choice::Card(s));
    finish_combat(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Battlefield, "the Bear died before dealing damage");
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
}

#[test]
fn cr_702_7_double_strike_deals_damage_twice() {
    let mut g = at_attack(&["Test Double"], &[]);
    let d = g.card(0, "Test Double", Zone::Battlefield);
    g.choose(Choice::Card(d));
    finish_combat(&mut g);
    assert_eq!(g.life(1), 16);

    // Blocked: its blocker dies in the first step; in the second it is
    // still blocked with no blocker, so it deals no damage (509.1h).
    let mut g = at_attack(&["Test Double"], &["Test Bear"]);
    let d = g.card(0, "Test Double", Zone::Battlefield);
    g.choose(Choice::Card(d));
    g.choose(Choice::Card(d));
    finish_combat(&mut g);
    assert_eq!(g.life(1), 20);
    assert_eq!(g.obj(d).zone, Zone::Battlefield);
}

#[test]
fn cr_702_15_lifelink() {
    let mut g = at_attack(&["Test Lifelinker"], &[]);
    let l = g.card(0, "Test Lifelinker", Zone::Battlefield);
    g.choose(Choice::Card(l));
    finish_combat(&mut g);
    assert_eq!((g.life(0), g.life(1)), (22, 18));
}

#[test]
fn cr_508_8_no_attackers_skips_blockers_and_damage() {
    let mut g = scen()
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Test Bear"])
        .at(Step::DeclareAttackers, 0)
        .start();
    g.pass(); // declare no attackers
    g.pass(); // P0 priority in the declare attackers step
    g.pass(); // P1
    assert_eq!(g.turn().step, Step::EndCombat);
}

#[test]
fn cr_506_4_blocked_creature_stays_blocked_after_its_blocker_leaves() {
    for (attacker, expected_life) in [("Test Bear", 20), ("Test Trampler", 16)] {
        let mut g = scen()
            .battlefield(0, &[attacker, "Mountain"])
            .hand(0, &["Lightning Bolt"])
            .battlefield(1, &["Test Bear"])
            .at(Step::DeclareAttackers, 0)
            .start();
        let a = g.card(0, attacker, Zone::Battlefield);
        g.choose(Choice::Card(a));
        // P0 could Bolt in the declare attackers step; passes.
        g.pass_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
        g.choose(Choice::Card(a));
        // Declare blockers step: P0 kills the blocker.
        assert_eq!(g.turn().step, Step::DeclareBlockers);
        let blocker = g.card(1, "Test Bear", Zone::Battlefield);
        let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(bolt));
        let t = g.tgt(blocker);
        g.choose(t);
        finish_combat(&mut g);
        assert_eq!(g.obj(blocker).zone, Zone::Graveyard);
        assert_eq!(g.life(1), expected_life, "{attacker}: 702.19e trample");
    }
}
