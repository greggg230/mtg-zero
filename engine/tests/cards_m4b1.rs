//! Cube cards supported in M4b-1 through the IR: removal with riders and
//! resolution checks, conditionals, counters to other zones, extra turns,
//! mana rocks and artifacts with activated abilities.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::mana::{ManaCost, B, C, U};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};

fn scen() -> Scenario {
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

/// Cast from hand choosing `targets`, paying with the first offered option
/// each time; stops at the caster's priority.
fn cast(g: &mut Game, p: u8, name: &str, targets: &[Choice]) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    for &t in targets {
        assert_eq!(g.kind(), Some(DecisionKind::Target), "{:?}", g.options_desc());
        g.choose(t);
    }
    while !at_priority(g) {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
}

#[test]
fn swords_to_plowshares_gives_life_equal_to_the_last_known_power() {
    // ruling: 518b1bf6
    let mut g = scen().battlefield(0, &["Plains"]).hand(0, &["Swords to Plowshares"]).battlefield(1, &["Test Giant"]).at(Step::Main1, 0).start();
    let giant = g.card(1, "Test Giant", Zone::Battlefield);
    let t = g.tgt(giant);

    cast(&mut g, 0, "Swords to Plowshares", &[t]);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Giant", Zone::Exile), 1);
    assert_eq!(g.life(1), 25, "its controller gains 5");
    assert_eq!(g.life(0), 20);
}

#[test]
fn fatal_push_checks_mana_value_on_resolution_and_revolt() {
    // Without revolt: mana value 3 survives.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Lotus Petal"])
        .hand(0, &["Fatal Push"])
        .battlefield(1, &["Test Double"])
        .at(Step::Main1, 0)
        .start();
    let dbl = g.card(1, "Test Double", Zone::Battlefield);
    let swamp = g.card(0, "Swamp", Zone::Battlefield);
    let fp = g.card(0, "Fatal Push", Zone::Hand);
    g.choose(Choice::Cast(fp));
    g.choose(g.tgt(dbl));
    g.choose(act(swamp, 0));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Double", Zone::Battlefield), 1);
    // Paying with Lotus Petal sacrifices it: a permanent left the
    // battlefield this turn, so mana value 4 or less dies.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Lotus Petal"])
        .hand(0, &["Fatal Push"])
        .battlefield(1, &["Test Double"])
        .at(Step::Main1, 0)
        .start();
    let dbl = g.card(1, "Test Double", Zone::Battlefield);
    let petal = g.card(0, "Lotus Petal", Zone::Battlefield);
    let fp = g.card(0, "Fatal Push", Zone::Hand);
    g.choose(Choice::Cast(fp));
    g.choose(g.tgt(dbl));
    g.choose(act(petal, 0));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Double", Zone::Graveyard), 1);
}

#[test]
fn unholy_heat_with_and_without_delirium() {
    let gy3 = ["Lightning Bolt", "Test Bear", "Forest"];
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Unholy Heat"])
        .graveyard(0, &gy3)
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let giant = g.card(1, "Test Giant", Zone::Battlefield);
    let t = g.tgt(giant);

    cast(&mut g, 0, "Unholy Heat", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(giant).damage, 2);
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Unholy Heat"])
        .graveyard(0, &["Lightning Bolt", "Test Bear", "Forest", "Mind Stone"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let giant = g.card(1, "Test Giant", Zone::Battlefield);
    let t = g.tgt(giant);

    cast(&mut g, 0, "Unholy Heat", &[t]);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Giant", Zone::Graveyard), 1, "6 damage with four card types");
}

#[test]
fn cabal_ritual_threshold_counts_on_resolution() {
    // Six cards: it is still on the stack as it resolves, so no threshold.
    let mut g =
        scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Cabal Ritual"]).graveyard(0, &["Island"; 6]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Cabal Ritual", &[]);
    settle(&mut g);
    assert_eq!(g.player(0).pool.amt[B], 3);
    let mut g =
        scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Cabal Ritual"]).graveyard(0, &["Island"; 7]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Cabal Ritual", &[]);
    settle(&mut g);
    assert_eq!(g.player(0).pool.amt[B], 5);
}

#[test]
fn abrupt_decay_cannot_be_countered() {
    let mut g = scen()
        .battlefield(0, &["Bayou"])
        .battlefield(0, &["Forest"])
        .hand(0, &["Abrupt Decay"])
        .battlefield(1, &["Island", "Island", "Mind Stone"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let stone = g.card(1, "Mind Stone", Zone::Battlefield);
    let t = g.tgt(stone);

    cast(&mut g, 0, "Abrupt Decay", &[t]);
    g.pass();
    let decay = g.card(0, "Abrupt Decay", Zone::Stack);
    let t = g.tgt(decay);

    cast(&mut g, 1, "Counterspell", &[t]);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Counterspell", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Mind Stone", Zone::Graveyard), 1, "Abrupt Decay resolved");
}

#[test]
fn oust_puts_the_creature_second_from_the_top() {
    let mut g = scen().battlefield(0, &["Plains"]).hand(0, &["Oust"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    let t = g.tgt(bear);

    cast(&mut g, 0, "Oust", &[t]);
    settle(&mut g);
    let lib = &g.player(1).library;
    assert_eq!(lib[lib.len() - 2], bear);
    assert!(g.knows(0, bear) && g.knows(1, bear));
    assert_eq!(g.life(1), 23);
}

#[test]
fn remand_and_no_more_lies_counter_to_hand_and_exile() {
    for (counter, lands, zone) in
        [("Remand", ["Island", "Island"], Zone::Hand), ("No More Lies", ["Tundra", "Tundra"], Zone::Exile)]
    {
        let mut g = scen()
            .battlefield(0, &["Forest", "Mountain"])
            .hand(0, &["Test Bear"])
            .battlefield(1, &lands)
            .hand(1, &[counter])
            .at(Step::Main1, 0)
            .start();
        cast(&mut g, 0, "Test Bear", &[]);
        g.pass();
        let bear = g.card(0, "Test Bear", Zone::Stack);
        let t = g.tgt(bear);

        cast(&mut g, 1, counter, &[t]);
        settle(&mut g);
        assert_eq!(g.count_in(0, "Test Bear", zone), 1, "{counter}");
        if counter == "Remand" {
            assert_eq!(g.player(1).hand.len(), 1, "Remand draws");
        }
    }
}

#[test]
fn cr_500_7_time_warp_on_the_opponent_keeps_the_turn_order() {
    let mut g = scen().battlefield(0, &["Island"; 5]).hand(0, &["Time Warp"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Time Warp", &[Choice::Target(Target::Player(1))]);
    settle(&mut g);
    let mut order = Vec::new();
    for n in 4..=6 {
        g.drive_until(|g| g.turn().number == n);
        order.push(g.turn().active);
    }
    // P1's extra turn, then P1's normal turn, then P0.
    assert_eq!(order, vec![1, 1, 0]);
}

#[test]
fn wheel_of_fortune_discards_hands_and_draws_seven() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 3])
        .hand(0, &["Wheel of Fortune", "Forest"])
        .hand(1, &["Plains", "Plains", "Plains"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Wheel of Fortune", &[]);
    settle(&mut g);
    assert_eq!((g.player(0).hand.len(), g.player(1).hand.len()), (7, 7));
    assert_eq!(g.count_in(1, "Plains", Zone::Graveyard), 3);
    assert_eq!(g.count_in(0, "Forest", Zone::Graveyard), 1);
}

#[test]
fn relic_of_sauron_mana_and_card_draw() {
    // Two mana in any combination of {U}, {B}, {R}: for {U}{B} only UB.
    let g = scen().battlefield(0, &["Relic of Sauron"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &ManaCost::parse("{U}{B}").unwrap()));
    assert!(!g.can_pay(0, &ManaCost::parse("{G}").unwrap()));
    // {3}, {T}: draw two, then discard a card (a decision).
    let mut g = scen()
        .battlefield(0, &["Relic of Sauron", "Island", "Island", "Island"])
        .hand(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let relic = g.card(0, "Relic of Sauron", Zone::Battlefield);
    g.choose(act(relic, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    assert_eq!(g.player(0).hand.len(), 3, "drew two, then discards");
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Card(bear));
    assert_eq!(g.player(0).hand.len(), 2);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
}

#[test]
fn zuran_orb_voltaic_key_and_aether_spellbomb() {
    let mut g = scen()
        .battlefield(0, &["Zuran Orb", "Forest", "Island"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let orb = g.card(0, "Zuran Orb", Zone::Battlefield);
    g.choose(act(orb, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    let forest = g.card(0, "Forest", Zone::Battlefield);
    g.choose(Choice::Card(forest));
    settle(&mut g);
    assert_eq!(g.life(0), 22);

    // Voltaic Key untaps a Sol Ring tapped for mana.
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Voltaic Key", "Island"])
        .hand(0, &["Test Zero"])
        .at(Step::Main1, 0)
        .start();
    let ring = g.card(0, "Sol Ring", Zone::Battlefield);
    let z = g.card(0, "Test Zero", Zone::Hand);
    g.choose(Choice::Cast(z));
    g.choose(act(ring, 0));
    // (The Ring's mana covers the rest: `PASS`, the pool pays; the Island
    // could pay instead, ENGINE.md 100.4.3.)
    if g.kind() == Some(DecisionKind::PayMana) {
        g.choose(Choice::Pass);
    }
    g.drive_until(at_priority);
    assert!(g.obj(ring).tapped);
    assert_eq!(g.player(0).pool.amt[C], 1);
    let key = g.card(0, "Voltaic Key", Zone::Battlefield);
    g.choose(act(key, 0)); // {1}, {T}: untap target artifact; the pool pays
    g.choose(g.tgt(ring));
    // (`PASS`: the pool pays; the Island could, ENGINE.md 100.4.3.)
    if g.kind() == Some(DecisionKind::PayMana) {
        g.choose(Choice::Pass);
    }
    g.drive_until(at_priority);
    assert_eq!(g.player(0).pool.amt[C], 0);
    settle(&mut g);
    assert!(!g.obj(ring).tapped);

    // Aether Spellbomb: {U}, sacrifice: return target creature.
    let mut g = scen()
        .battlefield(0, &["Aether Spellbomb", "Island"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bomb = g.card(0, "Aether Spellbomb", Zone::Battlefield);
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    g.choose(act(bomb, 0));
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Hand), 1);
    assert_eq!(g.player(0).pool.amt[U], 0);
}

#[test]
fn mox_amber_makes_colours_of_legends() {
    // ruling: 91e69158
    let g = scen().battlefield(0, &["Mox Amber", "Test Bear"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &ManaCost::parse("{1}").unwrap()), "no legendary creature or planeswalker");
    let g = scen().battlefield(0, &["Mox Amber", "Test Legend"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &ManaCost::parse("{W}").unwrap()));
    assert!(!g.can_pay(0, &ManaCost::parse("{U}").unwrap()));
}

#[test]
fn loran_destroys_up_to_one_and_taps_to_draw() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Loran of the Third Path"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Loran of the Third Path", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    // "Up to one": choosing none is allowed.
    assert!(g.has_option(Choice::Pass));
    let ring = g.card(1, "Sol Ring", Zone::Battlefield);
    g.choose(g.tgt(ring));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Graveyard), 1);
    let loran = g.card(0, "Loran of the Third Path", Zone::Battlefield);
    assert!(!g.has_option(act(loran, 0)), "summoning sick");
}
