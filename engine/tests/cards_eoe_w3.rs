//! Workstream W3 of the Edge of Eternities port (ENGINE.md 99.13): combat
//! and abilities, each card against its Oracle text. Test names cite the CR.

mod eoe_w3_kit;

use engine::db::{Counter, Kw};
use engine::{Choice, DecisionKind, Step, Zone};
use eoe_w3_kit::*;

// ---- Rust Harvester -----------------------------------------------------------------

/// Rust Harvester [1/1, menace]: "{2}, {T}, Exile an artifact card from your
/// graveyard: Put a +1/+1 counter on this creature, then it deals damage
/// equal to its power to any target" (CR 118.3, 602.2b: the card is chosen
/// as the cost is paid; only artifact cards).
#[test]
fn rust_harvester_exiles_an_artifact_card_from_your_graveyard_as_a_cost_cr602_2b() {
    let mut g = scen()
        .battlefield(0, &["Rust Harvester", "Mountain", "Mountain"])
        .graveyard(0, &["W3 Bear", "W3 Trinket", "W3 Robot", "Island"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let h = bf(&g, 0, "Rust Harvester");
    assert!(g.has_kw(h, Kw::Menace));
    let (bear, trinket, robot) =
        (g.card(0, "W3 Bear", Zone::Graveyard), g.card(0, "W3 Trinket", Zone::Graveyard), g.card(0, "W3 Robot", Zone::Graveyard));
    g.choose(act(h, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseGraveyardCard));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert!(opts.contains(&Choice::Card(trinket)) && opts.contains(&Choice::Card(robot)), "artifact cards: {opts:?}");
    assert!(!opts.contains(&Choice::Card(bear)), "not a creature card that isn't an artifact");
    assert_eq!(opts.len(), 2);
    g.choose(Choice::Card(robot));
    settle(&mut g);
    assert_eq!(g.obj(robot).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard);
    assert_eq!(g.counters_on(h, Counter::P1P1), 1);
    assert_eq!(g.life(1), 18, "2 damage: its power after the counter");
    assert!(g.obj(h).tapped);
}

/// Without an artifact card in its controller's graveyard the ability
/// can't be activated (CR 602.2b, 118.3); a token is no card (108.2).
#[test]
fn rust_harvester_needs_an_artifact_card_in_your_graveyard_cr118_3() {
    let g = scen()
        .battlefield(0, &["Rust Harvester", "Mountain", "Mountain"])
        .graveyard(0, &["W3 Bear", "Island"])
        .graveyard(1, &["W3 Trinket"])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let h = bf(&g, 0, "Rust Harvester");
    assert!(!g.has_option(act(h, 0)), "no artifact card in its controller's graveyard");
}

// ---- Sunstar Chaplain ---------------------------------------------------------------

/// Sunstar Chaplain [3/2]: "{2}, Remove a +1/+1 counter from a creature you
/// control: Tap target artifact or creature" (CR 118.3, 602.2b: any creature
/// you control with such a counter, chosen as the cost is paid).
#[test]
fn sunstar_chaplain_removes_a_counter_from_a_creature_you_control_cr118_3() {
    let mut g = scen()
        .battlefield(0, &["Sunstar Chaplain", "W3 Bear", "W3 Giant", "Plains", "Plains"])
        .battlefield(1, &["W3 Trinket", "W3 Wall"])
        .counters(0, "W3 Bear", Counter::P1P1, 2)
        .counters(0, "W3 Giant", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let (ch, bear, giant) = (bf(&g, 0, "Sunstar Chaplain"), bf(&g, 0, "W3 Bear"), bf(&g, 0, "W3 Giant"));
    let (trinket, wall) = (bf(&g, 1, "W3 Trinket"), bf(&g, 1, "W3 Wall"));
    assert_eq!((g.power(ch), g.toughness(ch)), (3, 2));
    g.choose(act(ch, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(trinket)) && g.has_option(g.tgt(wall)), "an artifact or a creature");
    g.choose(g.tgt(trinket));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    let opts: Vec<Choice> = g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect();
    assert_eq!(opts, vec![Choice::Card(bear), Choice::Card(giant)], "creatures you control with a +1/+1 counter");
    g.choose(Choice::Card(giant));
    settle(&mut g);
    assert!(g.obj(trinket).tapped, "{}", log(&g));
    assert_eq!(g.counters_on(giant, Counter::P1P1), 0);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 2);
    assert!(!g.obj(ch).tapped, "no {{T}} in the cost");
}

/// No creature of yours with a +1/+1 counter: the cost can't be paid (an
/// opponent's creature's counters don't count).
#[test]
fn sunstar_chaplain_needs_a_counter_on_a_creature_you_control_cr602_2b() {
    let g = scen()
        .battlefield(0, &["Sunstar Chaplain", "W3 Bear", "Plains", "Plains"])
        .battlefield(1, &["W3 Giant"])
        .counters(1, "W3 Giant", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let ch = bf(&g, 0, "Sunstar Chaplain");
    assert!(!g.has_option(act(ch, 0)));
}

/// "At the beginning of your end step, if you control two or more tapped
/// creatures, put a +1/+1 counter on target creature you control" (CR
/// 603.4: an intervening "if", checked as it triggers).
#[test]
fn sunstar_chaplain_end_step_counter_needs_two_tapped_creatures_cr603_4() {
    for tapped in [1usize, 2] {
        let mut g = scen().battlefield(0, &["Sunstar Chaplain", "W3 Bear", "W3 Giant"]).at(Step::Main1, 0).start();
        let (ch, bear, giant) = (bf(&g, 0, "Sunstar Chaplain"), bf(&g, 0, "W3 Bear"), bf(&g, 0, "W3 Giant"));
        g.set_tapped(bear, true);
        if tapped == 2 {
            g.set_tapped(giant, true);
        }
        to_end_step(&mut g);
        if tapped == 2 {
            g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
            g.choose(g.tgt(ch));
            settle(&mut g);
            assert_eq!(g.counters_on(ch, Counter::P1P1), 1, "{}", log(&g));
        } else {
            settle(&mut g);
            assert_eq!(g.counters_on(ch, Counter::P1P1), 0, "one tapped creature: no trigger\n{}", log(&g));
        }
    }
}
