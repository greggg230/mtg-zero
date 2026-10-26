//! State-based actions (CR 704) and game end (CR 104).

mod common;

use common::db;
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
    let new = (0..g.objs().len() as u16)
        .map(engine::CardIdx)
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
