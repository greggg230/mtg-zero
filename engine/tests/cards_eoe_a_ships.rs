//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): the Spacecraft
//! whose other abilities need nothing new, each against its Oracle text
//! (their station and threshold are in `cards_eoe_a.rs`).

mod eoe_a_kit;

use eoe_a_kit::*;
use engine::db::{types, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn cast(g: &mut Game, c: CardIdx) {
    g.choose(Choice::Cast(c));
    pay(g);
}

/// Cast `name` from P0's hand with the lands given, then answer the enter
/// trigger's decisions with `answer` (None: first option).
fn cast_ship(lands: &[&str], extra_bf: &[&str], opp_bf: &[&str], name: &str, answer: impl Fn(&Game) -> Option<Choice>) -> Game {
    let mut bfv: Vec<&str> = lands.to_vec();
    bfv.extend_from_slice(extra_bf);
    let mut g = scen().battlefield(0, &bfv).battlefield(1, opp_bf).hand(0, &[name]).at(Step::Main1, 0).start();
    let c = g.card(0, name, Zone::Hand);
    cast(&mut g, c);
    resolve_with(&mut g, answer);
    g
}

fn target_player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

/// "When this Spacecraft enters, put a +1/+1 counter on each creature you
/// control." (Not on the Greenhouse itself: not a creature yet.)
#[test]
fn atmospheric_greenhouse_counters_on_each_creature_you_control_cr122_6() {
    let g = cast_ship(&["Forest"; 5], &["Lane A Bear"], &["Lane A Giant"], "Atmospheric Greenhouse", |_| None);
    let (bear, giant) = (bf(&g, 0, "Lane A Bear"), bf(&g, 1, "Lane A Giant"));
    let ship = bf(&g, 0, "Atmospheric Greenhouse");
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert_eq!(g.counters_on(giant, Counter::P1P1), 0);
    assert_eq!(g.counters_on(ship, Counter::P1P1), 0);
}

/// "When this Spacecraft enters, it deals 3 damage to any target. ... {1}{R}:
/// This Spacecraft gets +2/+0 until end of turn." The pump can be activated
/// while it isn't a creature; it applies once it is one (CR 611.2c: locked
/// in; 722.2b).
#[test]
fn debris_field_crusher_deals_3_and_pumps_cr611_2c() {
    let g = cast_ship(&["Mountain"; 5], &[], &[], "Debris Field Crusher", |g| {
        (g.kind() == Some(DecisionKind::Target)).then(|| target_player(1))
    });
    assert_eq!(g.life(1), 17);
    let mut g = scen()
        .battlefield(0, &["Debris Field Crusher", "Mountain", "Mountain", "Lane A Giant", "Lane A Bear"])
        .counters(0, "Debris Field Crusher", charge(), 3)
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Debris Field Crusher");
    assert!(g.has_option(act(ship, 0)), "{{1}}{{R}}: +2/+0, not a creature");
    g.choose(act(ship, 0));
    pay(&mut g);
    settle(&mut g);
    let giant = bf(&g, 0, "Lane A Giant");
    station(&mut g, ship, giant);
    assert_eq!(g.counters_on(ship, charge()), 8);
    assert_eq!((g.power(ship), g.toughness(ship)), (3, 5));
    assert!(g.has_kw(ship, Kw::Flying));
}

/// "When this Spacecraft enters, destroy target noncreature permanent. Then
/// this Spacecraft deals 4 damage to each creature." (The Battleship itself
/// isn't a creature then.)
#[test]
fn extinguisher_battleship_destroys_then_deals_4_to_each_creature_cr120_3() {
    let g = cast_ship(&["Island"; 8], &["Lane A Bear"], &["Lane A Giant", "Lane A Totem"], "Extinguisher Battleship", |g| {
        (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bf(g, 1, "Lane A Totem")))
    });
    assert_eq!(g.count_in(1, "Lane A Totem", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Lane A Bear", Zone::Graveyard), 1);
    let giant = bf(&g, 1, "Lane A Giant");
    assert_eq!(g.objs()[giant.i()].damage, 4);
    let ship = bf(&g, 0, "Extinguisher Battleship");
    assert_eq!(g.objs()[ship.i()].damage, 0);
}

/// The target must be a noncreature permanent.
#[test]
fn extinguisher_battleship_targets_only_noncreature_permanents_cr115_1() {
    let mut g = scen()
        .battlefield(0, &["Island"; 8])
        .battlefield(1, &["Lane A Giant", "Lane A Totem"])
        .hand(0, &["Extinguisher Battleship"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Extinguisher Battleship", Zone::Hand);
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bf(&g, 1, "Lane A Totem"))));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Giant"))));
    assert!(!g.has_option(target_player(1)));
}

/// "When this Spacecraft enters, return target creature or enchantment card
/// from your graveyard to the battlefield."
#[test]
fn rescue_skiff_returns_a_creature_or_enchantment_card_cr115_1() {
    for pick in ["Lane A Bear", "Lane A Totem"] {
        let mut g = scen()
            .battlefield(0, &["Plains"; 6])
            .graveyard(0, &["Lane A Bear", "Lane A Totem", "Lane A Bolt"])
            .graveyard(1, &["Lane A Giant"])
            .hand(0, &["Rescue Skiff"])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, "Rescue Skiff", Zone::Hand);
        cast(&mut g, c);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        let bolt = g.card(0, "Lane A Bolt", Zone::Graveyard);
        let theirs = g.card(1, "Lane A Giant", Zone::Graveyard);
        assert!(!g.has_option(g.tgt(bolt)), "not an instant card");
        assert!(!g.has_option(g.tgt(theirs)), "your graveyard");
        let t = g.card(0, pick, Zone::Graveyard);
        g.choose(g.tgt(t));
        settle(&mut g);
        assert_eq!(g.count_in(0, pick, Zone::Battlefield), 1, "{pick}");
    }
}

/// "When this Spacecraft enters, each opponent sacrifices a nontoken
/// creature of their choice." A token creature can't be sacrificed to it.
#[test]
fn susurian_dirgecraft_each_opponent_sacrifices_a_nontoken_creature_cr701_21a() {
    let mut g = scen()
        .battlefield(1, &["Plains", "Plains", "Plains", "Plains", "Lane A Bear", "Lane A Giant"])
        .hand(1, &["Wedgelight Rammer"])
        .battlefield(0, &["Swamp"; 5])
        .hand(0, &["Susurian Dirgecraft"])
        .at(Step::Main1, 1)
        .start();
    // P1 makes a Robot token first.
    let r = g.card(1, "Wedgelight Rammer", Zone::Hand);
    cast(&mut g, r);
    settle(&mut g);
    next_main(&mut g, 0);
    let c = g.card(0, "Susurian Dirgecraft", Zone::Hand);
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    assert_eq!(g.pending_decision().unwrap().player, 1);
    let robot = g.battlefield().iter().copied().find(|&x| g.name(x) == "Robot").expect("the Robot token");
    assert!(!g.has_option(Choice::Card(robot)), "a token");
    let bear = bf(&g, 1, "Lane A Bear");
    assert!(g.has_option(Choice::Card(bear)) && g.has_option(Choice::Card(bf(&g, 1, "Lane A Giant"))));
    g.choose(Choice::Card(bear));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Lane A Bear", Zone::Graveyard), 1);
}

/// "When this Spacecraft enters, draw two cards, then discard a card."
#[test]
fn uthros_scanship_draws_two_then_discards_one_cr701_9a() {
    let mut g = scen().battlefield(0, &["Island"; 4]).hand(0, &["Uthros Scanship"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Uthros Scanship", Zone::Hand);
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    assert_eq!(g.player(0).hand.len(), 2);
    // (Two Islands: one option, the cards alike.)
    let first = g.pending_decision().unwrap().options[0].choice;
    g.choose(first);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
    assert_eq!(g.player(0).graveyard.len(), 1);
}

/// "When this Spacecraft enters, create a 2/2 colorless Robot artifact
/// creature token."
#[test]
fn wedgelight_rammer_creates_a_robot_cr111_2() {
    let g = cast_ship(&["Plains"; 4], &[], &[], "Wedgelight Rammer", |_| None);
    let robot = g.battlefield().iter().copied().find(|&x| g.name(x) == "Robot").expect("the Robot token");
    assert_eq!(g.types_of(robot), types::ARTIFACT | types::CREATURE);
    assert_eq!((g.power(robot), g.toughness(robot)), (2, 2));
    assert_eq!(g.colors_of(robot), 0);
    assert_eq!(g.objs()[robot.i()].controller, 0);
}

/// "When this Spacecraft enters, surveil 2." (CR 701.25a.)
#[test]
fn wurmwall_sweeper_surveils_two_cr701_25a() {
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &["Wurmwall Sweeper"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Wurmwall Sweeper", Zone::Hand);
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInGraveyard));
    // Both into the graveyard.
    for _ in 0..2 {
        let o = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
        g.choose(o);
        g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || g.stack().is_empty());
        if g.kind() != Some(DecisionKind::PutInGraveyard) {
            break;
        }
    }
    settle(&mut g);
    assert_eq!(g.player(0).graveyard.len(), 2, "{}", log(&g));
}
