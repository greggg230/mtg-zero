//! Lands (CR 305, 614.1c, 614.12) and land abilities.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::mana::{ManaCost, C};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};

fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 8]).library(1, &["Island"; 8])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn settle(g: &mut Game) {
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
}

fn play(g: &mut Game, p: u8, name: &str) -> CardIdx {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Play(c));
    c
}

#[test]
fn cr_614_1c_a_land_that_enters_tapped() {
    let mut g = manual().hand(0, &["Ketria Triome"]).at(Step::Main1, 0).start();
    let t = play(&mut g, 0, "Ketria Triome");
    assert!(g.obj(t).tapped);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Priority), Some(0)));
}

#[test]
fn cr_614_1c_fast_lands_count_the_other_lands_as_they_enter() {
    let mut g = manual().battlefield(0, &["Swamp", "Mountain"]).hand(0, &["Blackcleave Cliffs"]).at(Step::Main1, 0).start();
    let c = play(&mut g, 0, "Blackcleave Cliffs");
    assert!(!g.obj(c).tapped, "two other lands");
    let mut g = manual()
        .battlefield(0, &["Swamp", "Mountain", "Island"])
        .hand(0, &["Blackcleave Cliffs"])
        .at(Step::Main1, 0)
        .start();
    let c = play(&mut g, 0, "Blackcleave Cliffs");
    assert!(g.obj(c).tapped, "three other lands");
}

#[test]
fn cr_614_12_shock_lands_pay_two_life_or_enter_tapped() {
    let mut g = manual().hand(0, &["Blood Crypt"]).at(Step::Main1, 0).start();
    let c = play(&mut g, 0, "Blood Crypt");
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(0)));
    g.choose(Choice::Yes);
    assert!(!g.obj(c).tapped);
    assert_eq!(g.life(0), 18);
    assert!(at_priority(&g));

    let mut g = manual().hand(0, &["Blood Crypt"]).at(Step::Main1, 0).start();
    let c = play(&mut g, 0, "Blood Crypt");
    g.choose(Choice::Pass);
    assert!(g.obj(c).tapped);
    assert_eq!(g.life(0), 20);

    // CR 119.4: at 1 life the 2 life cannot be paid.
    let mut g = manual().hand(0, &["Blood Crypt"]).life(0, 1).at(Step::Main1, 0).start();
    play(&mut g, 0, "Blood Crypt");
    assert_eq!(g.options_desc(), vec!["0=pass".to_string()]);
    // At exactly 2 it can (and the player then loses).
    let mut g = manual().hand(0, &["Blood Crypt"]).life(0, 2).at(Step::Main1, 0).start();
    play(&mut g, 0, "Blood Crypt");
    g.choose(Choice::Yes);
    assert!(g.is_over());
}

#[test]
fn starting_town_enters_untapped_in_your_first_three_turns() {
    // Turn 3, player 0 started: their second turn.
    let mut g = manual().hand(0, &["Starting Town"]).at(Step::Main1, 0).start();
    let t = play(&mut g, 0, "Starting Town");
    assert!(!g.obj(t).tapped);
    // Turn 7: their fourth.
    let mut g = manual().hand(0, &["Starting Town"]).turn_number(7).at(Step::Main1, 0).start();
    let t = play(&mut g, 0, "Starting Town");
    assert!(g.obj(t).tapped);
    // {T}: Add {C}; {T}, Pay 1 life: Add one mana of any color.
    let g = manual().battlefield(0, &["Starting Town"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &ManaCost::parse("{C}").unwrap()));
    assert!(g.can_pay(0, &ManaCost::parse("{U}").unwrap()));
}

#[test]
fn horizon_lands_cost_life_for_mana_and_cycle_themselves() {
    let mut g =
        manual().battlefield(0, &["Horizon Canopy", "Island"]).hand(0, &["Test Bear"]).at(Step::Main1, 0).start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    g.drive_until(at_priority);
    assert_eq!(g.life(0), 19, "{{T}}, Pay 1 life: Add {{G}} or {{W}}");
    settle(&mut g);
    // {1}, {T}, sacrifice: draw a card.
    let mut g = manual().battlefield(0, &["Horizon Canopy", "Island"]).at(Step::Main1, 0).start();
    let hc = g.card(0, "Horizon Canopy", Zone::Battlefield);
    g.choose(act(hc, 1));
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Horizon Canopy", Zone::Graveyard), 1);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
    assert_eq!(g.life(0), 20);
}

#[test]
fn gaeas_cradle_and_ancient_tomb() {
    // Cradle: {G} per creature; none -> no mana.
    let g = manual().battlefield(0, &["Gaea's Cradle"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &ManaCost::parse("{G}").unwrap()));
    let g = manual().battlefield(0, &["Gaea's Cradle", "Test Bear", "Test Wall"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &ManaCost::parse("{G}{G}").unwrap()));
    assert!(!g.can_pay(0, &ManaCost::parse("{G}{G}{G}").unwrap()));
    // Ancient Tomb: {C}{C} and 2 damage to you; the extra {C} floats.
    let mut g = manual().battlefield(0, &["Ancient Tomb"]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
    let z = g.card(0, "Test Zero", Zone::Hand);
    g.choose(Choice::Cast(z));
    g.drive_until(at_priority);
    assert_eq!(g.life(0), 18);
    assert_eq!(g.player(0).pool.amt[C], 1);
}

#[test]
fn wasteland_and_karakas() {
    let mut g = manual()
        .battlefield(0, &["Wasteland", "Karakas"])
        .battlefield(1, &["Forest", "Tropical Island", "Test Legend", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let wl = g.card(0, "Wasteland", Zone::Battlefield);
    let forest = g.card(1, "Forest", Zone::Battlefield);
    let trop = g.card(1, "Tropical Island", Zone::Battlefield);
    // Wasteland: ability 0 is {T}: Add {C}; ability 1 destroys a nonbasic land.
    g.choose(act(wl, 1));
    assert!(!g.has_option(g.tgt(forest)) && g.has_option(g.tgt(trop)));
    g.choose(g.tgt(trop));
    g.drive_until(at_priority);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Tropical Island", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Wasteland", Zone::Graveyard), 1);
    // Karakas: {T}: Add {W}; {T}: return target legendary creature.
    let k = g.card(0, "Karakas", Zone::Battlefield);
    g.choose(act(k, 1));
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    assert!(!g.has_option(g.tgt(bear)));
    let legend = g.card(1, "Test Legend", Zone::Battlefield);
    g.choose(g.tgt(legend));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Legend", Zone::Hand), 1);
}

#[test]
fn strip_mine_is_blocked_by_its_missing_mana_ability() {
    let err = db().expect_id("Strip Mine").unwrap_err();
    assert!(err.contains("mana_only_from_produces"), "{err}");
}
