//! Card tests of W6's counter cards (ENGINE.md 99.16): Loading Zone's
//! counter-doubling replacement (CR 614.1a, 122.6), against its Oracle text
//! and its rulings of 2025-07-25.

mod eoe_a_kit;

use engine::db::Counter;
use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_a_kit::*;

fn cast_on(g: &mut Game, p: u8, name: &str, target: Option<engine::CardIdx>) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    if let Some(t) = target {
        if g.kind() == Some(DecisionKind::Target) {
            g.choose(g.tgt(t));
        }
    }
    pay(g);
    settle(g);
}

/// Loading Zone: "If one or more counters would be put on a creature,
/// Spacecraft, or Planet you control, twice that many of each of those kinds
/// of counters are put on it instead." Honor's +1/+1 counter on a creature
/// you control: two; on an opponent's: one. Two Loading Zones: four (the
/// ruling: "four times the original number").
#[test]
fn cr_614_1a_loading_zone_doubles_counters_on_your_creatures() {
    for (zones, mine, want) in [(1usize, true, 2u32), (1, false, 1), (2, true, 4), (0, true, 1)] {
        let mut bf0: Vec<&str> = vec!["Plains", "Lane A Bear"];
        bf0.extend(std::iter::repeat_n("Loading Zone", zones));
        let mut g = scen().battlefield(0, &bf0).battlefield(1, &["Lane A Bear"]).hand(0, &["Honor"]).at(Step::Main1, 0).start();
        let bear = bf(&g, if mine { 0 } else { 1 }, "Lane A Bear");
        cast_on(&mut g, 0, "Honor", Some(bear));
        assert_eq!(g.counters_on(bear, Counter::P1P1), want, "zones {zones} mine {mine}\n{}", log(&g));
    }
}

/// "If a creature, Spacecraft, or Planet you control would enter the
/// battlefield with a number of counters on it, it enters with twice that
/// many instead" (ruling): Dockworker Drone ("enters with a +1/+1 counter")
/// enters with two.
#[test]
fn cr_122_6_loading_zone_doubles_counters_a_creature_enters_with() {
    let mut g = scen().battlefield(0, &["Plains", "Plains", "Loading Zone"]).hand(0, &["Dockworker Drone"]).at(Step::Main1, 0).start();
    cast_on(&mut g, 0, "Dockworker Drone", None);
    let d = bf(&g, 0, "Dockworker Drone");
    assert_eq!(g.counters_on(d, Counter::P1P1), 2, "{}", log(&g));
}

/// Every kind of counter, on a Spacecraft (a noncreature artifact below its
/// threshold) and on a Planet (a land): station's charge counters (CR
/// 702.184a) are doubled; a noncreature artifact that is neither gets none
/// more (Lane A Trinket: no counters are put on it here, so the Spacecraft
/// stands for the recipients' filter).
#[test]
fn cr_702_184a_loading_zone_doubles_charge_counters_on_spacecraft_and_planets() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Planet", "Lane A Bear", "Lane A Giant", "Loading Zone"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let planet = bf(&g, 0, "Lane A Planet");
    let (bear, giant) = (bf(&g, 0, "Lane A Bear"), bf(&g, 0, "Lane A Giant"));
    station(&mut g, ship, bear);
    assert_eq!(g.counters_on(ship, charge()), 4, "2 doubled\n{}", log(&g));
    station(&mut g, planet, giant);
    assert_eq!(g.counters_on(planet, charge()), 10, "5 doubled\n{}", log(&g));
}
