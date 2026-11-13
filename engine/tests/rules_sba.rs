//! State-based actions (CR 704) and game end (CR 104).

mod common;

use common::db;
use engine::db::Kw;
use engine::game::EndReason;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Target, Zone};

/// Pass priority until the stack is empty (or a non-priority decision).
fn settle(g: &mut Game) {
    g.pass_until(|g| g.stack().is_empty());
}

fn cast(g: &mut Game, p: u8, name: &str, targets: &[Choice]) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    for &t in targets {
        g.choose(t);
    }
}

#[test]
fn cr_704_5a_player_at_zero_life_loses() {
    let mut g = Scenario::new(db())
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .life(1, 3)
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Lightning Bolt", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    let o = g.outcome().expect("game over");
    assert_eq!(o.winner, Some(0));
    assert_eq!(o.reason, EndReason::Sba);
    assert!(g.to_move().is_none());
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    assert!(v.is_empty());
}

#[test]
fn cr_704_5b_drawing_from_an_empty_library_loses() {
    let mut g = Scenario::new(db()).library(0, &["Island"; 3]).at(Step::End, 0).start();
    // P0's end step, cleanup, then P1's turn: P1 draws from an empty library.
    g.pass_until(|g| g.is_over());
    let o = g.outcome().unwrap();
    assert_eq!(o.winner, Some(0));
    assert!(g.player(1).lost);
    assert_eq!(g.turn().number, 4);
    assert_eq!(g.turn().step, Step::Draw);
}

#[test]
fn cr_104_4a_all_players_losing_at_once_is_a_draw() {
    let g = Scenario::new(db()).life(0, 0).life(1, -2).at(Step::Main1, 0).start();
    let o = g.outcome().unwrap();
    assert_eq!(o.winner, None);
}

#[test]
fn cr_704_5f_zero_toughness_goes_to_the_graveyard() {
    let mut g = Scenario::new(db()).battlefield(0, &["Island"]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Test Zero", &[]);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Zero", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Test Zero", Zone::Battlefield), 0);
}

#[test]
fn cr_704_5g_lethal_damage_and_cr_514_2_damage_wears_off() {
    let mut g = Scenario::new(db())
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .battlefield(1, &["Test Trampler"])
        .library(0, &["Island"; 3])
        .library(1, &["Island"; 3])
        .at(Step::Main1, 0)
        .start();
    let tr = g.card(1, "Test Trampler", Zone::Battlefield);
    let t = g.tgt(tr);
    cast(&mut g, 0, "Lightning Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(tr).damage, 3);
    assert_eq!(g.obj(tr).zone, Zone::Battlefield, "3 damage on a 4/4 is not lethal");
    // Damage wears off in the cleanup step.
    g.pass_until(|g| g.turn().number == 4);
    assert_eq!(g.obj(tr).damage, 0);

    // Two Bolts in one turn kill it.
    let mut g = Scenario::new(db())
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .battlefield(1, &["Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let tr = g.card(1, "Test Trampler", Zone::Battlefield);
    let t = g.tgt(tr);
    cast(&mut g, 0, "Lightning Bolt", &[t]);
    settle(&mut g);
    let t = g.tgt(tr);
    cast(&mut g, 0, "Lightning Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Trampler", Zone::Graveyard), 1);
}

#[test]
fn cr_704_5h_deathtouch_damage_destroys() {
    let mut g = Scenario::new(db())
        .battlefield(0, &["Test Trampler"])
        .battlefield(1, &["Test Deathtouch"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let tr = g.card(0, "Test Trampler", Zone::Battlefield);
    let dt = g.card(1, "Test Deathtouch", Zone::Battlefield);
    g.choose(Choice::Card(tr)); // the only attacker: declaring ends by itself
    assert_eq!(g.kind(), Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(tr)); // the deathtouch creature blocks
    // Trample with one blocker: 1 is lethal (deathtouch), 1..=4 to it.
    while g.kind() == Some(DecisionKind::AssignDamage) {
        g.choose(Choice::Number(1));
    }
    g.pass_until(|g| g.turn().step == Step::EndCombat);
    assert_eq!(g.count_in(0, "Test Trampler", Zone::Graveyard), 1, "1 deathtouch damage destroys a 4/4");
    assert_eq!(g.count_in(1, "Test Deathtouch", Zone::Graveyard), 1);
    assert_eq!(g.obj(dt).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 17, "trample: 3 excess damage to the player");
}

/// CR 704.5h destroys a creature for damage dealt by a source with
/// deathtouch "since the last time state-based actions were checked", and a
/// check that performs nothing is still a check (704.3). Test Indestructible
/// blocks Test Giant (5/5 trample, deathtouch) and is dealt 1 deathtouch
/// damage: the check after combat damage performs nothing (indestructible).
/// Then it loses all its abilities (The Wondrous Wasp's trigger), and the
/// next check doesn't destroy it: a 2/2 with 1 damage marked, dealt no
/// deathtouch damage since the last check (the rules check's F1: the record
/// outlived a check that performed nothing; ENGINE.md 151.1).
#[test]
fn cr_704_5h_deathtouch_damage_before_the_last_check_does_not_destroy() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Island"; 5])
        .battlefield(0, &["Test Giant", "Island", "Island"])
        .hand(0, &["The Wondrous Wasp"])
        .battlefield(1, &["Test Indestructible"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let giant = g.card(0, "Test Giant", Zone::Battlefield);
    let ind = g.card(1, "Test Indestructible", Zone::Battlefield);
    g.choose(Choice::Card(giant));
    g.pass(); // no more attackers
    g.pass_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(giant));
    // Trample and deathtouch: 1 is lethal; 1 to the blocker, 4 to P1.
    g.pass_until(|g| g.kind() == Some(DecisionKind::AssignDamage));
    g.choose(Choice::Number(1));
    while g.kind() == Some(DecisionKind::AssignDamage) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!((g.turn().step, g.kind()), (Step::CombatDamage, Some(DecisionKind::Priority)), "{}", g.log_lines().join("\n"));
    assert_eq!((g.obj(ind).zone, g.obj(ind).damage, g.life(1)), (Zone::Battlefield, 1, 16));
    // The Wondrous Wasp (flash): "tap up to one target creature. It loses
    // all abilities for as long as The Wondrous Wasp remains on the
    // battlefield."
    let wasp = g.card(0, "The Wondrous Wasp", Zone::Hand);
    g.choose(Choice::Cast(wasp));
    for _ in 0..100 {
        if g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        match g.kind() {
            Some(DecisionKind::Priority) => {
                g.pass();
            }
            Some(DecisionKind::Target) => {
                let t = g.tgt(ind);
                g.choose(t);
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    assert!(g.obj(ind).tapped && !g.has_kw(ind, Kw::Indestructible), "{}", g.log_lines().join("\n"));
    assert_eq!(g.obj(ind).zone, Zone::Battlefield, "destroyed for deathtouch damage dealt before the last check\n{}", g.log_lines().join("\n"));
    assert_eq!(g.obj(ind).damage, 1);
}

#[test]
fn cr_704_5j_legend_rule_controller_chooses() {
    let mut g = Scenario::new(db())
        .battlefield(0, &["Test Legend", "Plains", "Plains"])
        .hand(0, &["Test Legend"])
        .at(Step::Main1, 0)
        .start();
    let old = g.card(0, "Test Legend", Zone::Battlefield);
    cast(&mut g, 0, "Test Legend", &[]);
    settle(&mut g);
    assert_eq!(g.kind(), Some(DecisionKind::LegendRule));
    assert_eq!(g.to_move(), Some(0));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2);
    let new = (0..g.objs().len() as u32)
        .map(engine::CardIdx::new)
        .find(|&c| c != old && g.obj(c).zone == Zone::Battlefield && g.name(c) == "Test Legend")
        .unwrap();
    g.choose(Choice::Card(new));
    assert_eq!(g.obj(old).zone, Zone::Graveyard);
    assert_eq!(g.obj(new).zone, Zone::Battlefield);
}

#[test]
fn turn_limit_is_a_draw() {
    let db = db();
    let deck: Vec<u16> = vec![db.id("Island").unwrap(); 40];
    let spec = engine::GameSpec { mulligans: false, max_turns: 6, ..Default::default() };
    let mut g = Game::new(db, spec, [&deck, &deck], 3).unwrap();
    g.play_first();
    g.pass_until(|g| g.is_over());
    assert_eq!(g.outcome().unwrap().reason, EndReason::TurnLimit);
    assert_eq!(g.outcome().unwrap().winner, None);
}
