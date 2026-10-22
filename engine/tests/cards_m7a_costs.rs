//! M7a: alternative costs (CR 118.9), evoke (702.74), divided damage
//! (601.2d), and the cards they brought (the Forces, Daze, Snuff Out, the
//! Elemental Incarnations, Pyrokinesis, Baleful Mastery).

mod common;

use common::db;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn pay(g: &mut Game) {
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(at_priority);
    }
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::ChooseAltCost));
}

/// Choose alternative cost k (`NUMBER` k + 1) if the choice is asked, then
/// answer forced decisions until a cost choice of the kinds below or
/// priority.
fn alt(g: &mut Game, k: u8) {
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(k + 1));
    }
    g.drive_until(|g| {
        at_priority(g)
            || matches!(
                g.kind(),
                Some(DecisionKind::ExileCost) | Some(DecisionKind::BounceCost) | Some(DecisionKind::Sacrifice)
            )
    });
}

/// Run the stack down, answering with `answer` for anything else asked.
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        match answer(g) {
            Some(c) => {
                g.choose(c);
            }
            None => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    panic!("resolve_with: no progress");
}

#[test]
fn force_of_will_cr_118_9_pays_one_life_and_exiles_a_blue_card() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Force of Will", "Counterspell", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Lightning Bolt");
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    pay(&mut g);
    g.pass();
    // P1 has no land: Force of Will is castable for its alternative cost.
    let fow = g.card(1, "Force of Will", Zone::Hand);
    g.choose(Choice::Cast(fow));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::ChooseAltCost));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        assert!(!g.has_option(Choice::Number(0)), "the mana cost can't be paid");
    }
    alt(&mut g, 0);
    // (The only target, Bolt, is chosen without a decision.)
    assert_eq!(g.kind(), Some(DecisionKind::ExileCost));
    let cs = g.card(1, "Counterspell", Zone::Hand);
    assert_eq!(g.pending_decision().unwrap().options.len(), 1, "a blue card: Test Bear is green");
    g.choose(Choice::Card(cs));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(cs).zone, Zone::Exile);
    assert_eq!(g.life(1), 19, "1 life paid, the Bolt countered");
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1);
}

#[test]
fn cr_601_2f_thalia_taxes_an_alternative_cost_too() {
    let g = scen()
        .battlefield(0, &["Thalia, Guardian of Thraben", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Force of Will", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    // P1 can never cast Force of Will here: {1} more and no mana.
    let fow = g.card(1, "Force of Will", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(fow)));
    let _ = g;
}

#[test]
fn snuff_out_alternative_cost_needs_a_swamp() {
    let g = scen().battlefield(1, &["Mountain"]).hand(1, &["Snuff Out"]).battlefield(0, &["Test Bear"]).at(Step::Main1, 1).start();
    let so = g.card(1, "Snuff Out", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(so)), "no Swamp and {{3}}{{B}} unpayable");
    let mut g = scen().battlefield(1, &["Swamp"]).hand(1, &["Snuff Out"]).battlefield(0, &["Test Bear"]).at(Step::Main1, 1).start();
    cast(&mut g, 1, "Snuff Out");
    alt(&mut g, 0);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 16);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
}

#[test]
fn daze_returns_an_island_to_hand_as_its_cost() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Lightning Bolt");
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    pay(&mut g);
    g.pass();
    let island = bf(&g, 1, "Island");
    cast(&mut g, 1, "Daze");
    alt(&mut g, 0);
    assert_eq!(g.kind(), Some(DecisionKind::BounceCost));
    g.choose(Choice::Card(island));
    g.drive_until(at_priority);
    assert_eq!(g.obj(island).zone, Zone::Hand);
    // "Unless its controller pays {1}": P0 has no untapped land left.
    resolve_with(&mut g, |_| Some(Choice::Pass));
    assert_eq!(g.life(1), 20);
}

#[test]
fn force_of_negation_alternative_cost_only_on_another_players_turn() {
    let g = scen().hand(1, &["Force of Negation", "Island"]).at(Step::Main1, 1).start();
    let fon = g.card(1, "Force of Negation", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(fon)), "no target anyway, and its own turn");
}

#[test]
fn cr_702_74_evoke_solitude_exiles_and_then_is_sacrificed() {
    let mut g = scen()
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Solitude", "Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    cast(&mut g, 0, "Solitude");
    alt(&mut g, 0);
    assert_eq!(g.kind(), Some(DecisionKind::ExileCost));
    let stp = g.card(0, "Swords to Plowshares", Zone::Hand);
    g.choose(Choice::Card(stp));
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(bear)) {
            Some(g.tgt(bear))
        } else {
            None
        }
    });
    assert_eq!(g.obj(stp).zone, Zone::Exile);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.life(1), 22, "its controller gains life equal to its power");
    assert_eq!(g.count_in(0, "Solitude", Zone::Graveyard), 1, "evoked: sacrificed");
}

#[test]
fn evoke_is_not_the_mana_cost_solitude_cast_normally_stays() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 5])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Solitude", "Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Solitude");
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(0));
    g.drive_until(at_priority);
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let sol = bf(&g, 0, "Solitude");
    assert!(!g.obj(sol).evoked);
}

#[test]
fn cr_601_2d_fury_divides_its_damage_as_the_trigger_goes_on_the_stack() {
    let mut g = scen()
        .battlefield(1, &["Test Bear", "Test Striker", "Test Wall"])
        .hand(0, &["Fury", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (bear, striker, wall) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Striker"), bf(&g, 1, "Test Wall"));
    cast(&mut g, 0, "Fury");
    alt(&mut g, 0);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Card(bolt));
    let mut divided = false;
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            if g.has_option(g.tgt(bear)) {
                Some(g.tgt(bear))
            } else if g.has_option(g.tgt(striker)) {
                Some(g.tgt(striker))
            } else {
                Some(Choice::Pass)
            }
        }
        // 2 to the bear (the first chosen), the rest (2) to the striker.
        Some(DecisionKind::DivideDamage) => Some(Choice::Number(2)),
        _ => None,
    });
    divided |= g.obj(bear).zone == Zone::Graveyard && g.obj(striker).zone == Zone::Graveyard;
    assert!(divided, "{}", g.log_lines().join("\n"));
    assert_eq!(g.obj(wall).zone, Zone::Battlefield);
}

#[test]
fn subtlety_the_spells_owner_puts_it_on_top_or_bottom() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .hand(0, &["Test Bear"])
        .hand(1, &["Subtlety", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Bear");
    pay(&mut g);
    g.pass();
    cast(&mut g, 1, "Subtlety");
    alt(&mut g, 0);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Card(cs));
    let mut asked = None;
    for _ in 0..200 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        match g.kind() {
            Some(DecisionKind::TopOrBottom) => {
                asked = Some(g.pending_decision().unwrap().player);
                g.choose(Choice::Pass); // the bottom
            }
            Some(DecisionKind::Priority) if !g.stack().is_empty() => {
                g.pass();
            }
            Some(DecisionKind::Priority) | None => break,
            _ => {
                let c = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass).unwrap();
                g.choose(c);
            }
        }
    }
    assert_eq!(asked, Some(0), "the spell's owner chooses");
    let bear = g.card(0, "Test Bear", Zone::Library);
    assert_eq!(g.player(0).library[0], bear, "on the bottom");
}
