//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): more of the set's
//! cards against their Oracle text, and lane A's effects that only
//! cross-lane cards use, on custom IR test cards (those cards are finished
//! after the lanes merge, ENGINE.md 99.6). Test names cite the CR.

mod eoe_a_kit;

use eoe_a_kit::*;
use engine::db::{Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};

fn cast(g: &mut Game, c: CardIdx) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
}

// ---- Emissary Escort -------------------------------------------------------

/// "This creature gets +X/+0, where X is the greatest mana value among other
/// artifacts you control." (A static ability, layer 7c, CR 613.4c: counted
/// continuously; not the Escort itself, not the opponent's.)
#[test]
fn emissary_escort_gets_the_greatest_mana_value_of_your_other_artifacts_cr613_4c() {
    let mut g = scen()
        .battlefield(0, &["Emissary Escort", "Lane A Trinket", "Lane A Robot", "Mountain"])
        .battlefield(1, &["Extinguisher Battleship"])
        .hand(0, &["Lane A Bolt"])
        .at(Step::Main1, 0)
        .start();
    let e = bf(&g, 0, "Emissary Escort");
    assert_eq!((g.power(e), g.toughness(e)), (2, 4), "the Robot's 2, not the opponent's 8");
    // The Robot dies: the Trinket's 1 (continuously, CR 611.3a).
    let bolt = g.card(0, "Lane A Bolt", Zone::Hand);
    let robot = bf(&g, 0, "Lane A Robot");
    cast(&mut g, bolt);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(robot));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.power(e), 1);
    let g = scen().battlefield(0, &["Emissary Escort"]).battlefield(1, &["Extinguisher Battleship"]).at(Step::Main1, 0).start();
    let e = bf(&g, 0, "Emissary Escort");
    assert_eq!(g.power(e), 0, "no other artifact you control");
    let g = scen().battlefield(0, &["Emissary Escort", "Emissary Escort"]).at(Step::Main1, 0).start();
    let e = bf(&g, 0, "Emissary Escort");
    assert_eq!(g.power(e), 2, "the other Escort (mana value 2)");
}

// ---- Pulsar Squadron Ace ---------------------------------------------------

fn ace_game(library: &[&str]) -> Game {
    let mut g = scen().battlefield(0, &["Plains", "Plains"]).library(0, library).hand(0, &["Pulsar Squadron Ace"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Pulsar Squadron Ace", Zone::Hand);
    cast(&mut g, c);
    pay(&mut g);
    g
}

/// "When this creature enters, look at the top five cards of your library.
/// You may reveal a Spacecraft card from among them and put it into your
/// hand. Put the rest on the bottom of your library in a random order. If you
/// didn't put a card into your hand this way, put a +1/+1 counter on this
/// creature."
#[test]
fn pulsar_squadron_ace_takes_a_spacecraft_or_gets_a_counter_cr701_16a() {
    let lib = ["Lane A Bear", "Wurmwall Sweeper", "Lane A Bear", "Lane A Bear", "Lane A Bear", "Island"];
    // Take the Sweeper: no counter.
    let mut g = ace_game(&lib);
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInHand));
    let o = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
    assert!(matches!(o, Choice::Card(c) if g.name(c) == "Wurmwall Sweeper"), "only the Spacecraft");
    g.choose(o);
    settle(&mut g);
    let ace = bf(&g, 0, "Pulsar Squadron Ace");
    assert_eq!(g.count_in(0, "Wurmwall Sweeper", Zone::Hand), 1);
    assert_eq!(g.counters_on(ace, Counter::P1P1), 0);
    // The rest on the bottom: the Island is the top card now.
    let top = *g.player(0).library.last().unwrap();
    assert_eq!(g.name(top), "Island");
    // Decline: a counter.
    let mut g = ace_game(&lib);
    g.drive_until(|g| g.kind() == Some(DecisionKind::PutInHand));
    g.choose(Choice::Pass);
    settle(&mut g);
    let ace = bf(&g, 0, "Pulsar Squadron Ace");
    assert_eq!(g.counters_on(ace, Counter::P1P1), 1);
    assert_eq!(g.player(0).hand.len(), 0);
    // No Spacecraft among them: a counter (the decision is still asked, a
    // pass only: what the top cards are is hidden from the opponent).
    let mut g = ace_game(&["Lane A Bear"; 6]);
    resolve_with(&mut g, |_| Some(Choice::Pass));
    let ace = bf(&g, 0, "Pulsar Squadron Ace");
    assert_eq!(g.counters_on(ace, Counter::P1P1), 1);
}

// ---- Selfcraft Mechan ------------------------------------------------------

/// "When this creature enters, you may sacrifice an artifact. When you do,
/// put a +1/+1 counter on target creature and draw a card." (A reflexive
/// triggered ability, CR 603.12: its target is chosen after the sacrifice;
/// the Mechan may sacrifice itself.)
#[test]
fn selfcraft_mechan_sacrifices_an_artifact_for_a_reflexive_trigger_cr603_12() {
    for take in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Island"; 4])
            .battlefield(0, &["Lane A Trinket", "Lane A Bear"])
            .hand(0, &["Selfcraft Mechan"])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, "Selfcraft Mechan", Zone::Hand);
        cast(&mut g, c);
        pay(&mut g);
        g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
        if !take {
            g.choose(Choice::Pass);
            settle(&mut g);
            assert_eq!(g.count_in(0, "Lane A Trinket", Zone::Battlefield), 1);
            assert_eq!(g.player(0).hand.len(), 0);
            continue;
        }
        g.choose(Choice::Yes);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
        let mech = bf(&g, 0, "Selfcraft Mechan");
        assert!(g.has_option(Choice::Card(mech)), "itself, an artifact");
        assert!(!g.has_option(Choice::Card(bf(&g, 0, "Lane A Bear"))), "an artifact");
        g.choose(Choice::Card(bf(&g, 0, "Lane A Trinket")));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        let bear = bf(&g, 0, "Lane A Bear");
        g.choose(g.tgt(bear));
        settle(&mut g);
        assert_eq!(g.count_in(0, "Lane A Trinket", Zone::Graveyard), 1);
        assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
        assert_eq!(g.player(0).hand.len(), 1);
    }
}

// ---- cross-lane effects on custom cards --------------------------------------

/// "Tap target artifact or creature." (Mental Modulation, Sunstar Chaplain.)
#[test]
fn tap_target_artifact_or_creature_cr701_26a() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Lane A Trinket", "Lane A Totem"])
        .hand(0, &["Lane A Tap"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Lane A Tap", Zone::Hand);
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Totem"))), "an enchantment");
    let t = bf(&g, 1, "Lane A Trinket");
    g.choose(g.tgt(t));
    pay(&mut g);
    settle(&mut g);
    assert!(g.obj(t).tapped);
}

/// "Target artifact or creature gains indestructible until end of turn."
/// (Reroute Systems.)
#[test]
fn pump_target_artifact_or_creature_until_end_of_turn_cr611_2a() {
    let mut g = scen().battlefield(0, &["Plains", "Lane A Trinket"]).hand(0, &["Lane A Shield"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Lane A Shield", Zone::Hand);
    let t = bf(&g, 0, "Lane A Trinket");
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(t));
    pay(&mut g);
    settle(&mut g);
    assert!(g.has_kw(t, Kw::Indestructible));
    next_main(&mut g, 1);
    assert!(!g.has_kw(t, Kw::Indestructible));
}

/// "Put two +1/+1 counters on target creature you control. It gains lifelink
/// until end of turn." (Archenemy's Charm's third mode.)
#[test]
fn counters_and_a_keyword_on_target_creature_you_control_cr122_6() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Lane A Bear"])
        .battlefield(1, &["Lane A Giant"])
        .hand(0, &["Lane A Charm"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Lane A Charm", Zone::Hand);
    let bear = bf(&g, 0, "Lane A Bear");
    cast(&mut g, c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Lane A Giant"))), "you control");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 2);
    assert!(g.has_kw(bear, Kw::Lifelink));
}

/// "As an additional cost to cast this spell, sacrifice an artifact or
/// creature." (Embrace Oblivion, Scrounge for Eternity; CR 601.2b, 601.2h.)
/// Without one it can't be cast.
#[test]
fn additional_cost_sacrifice_an_artifact_or_creature_cr601_2h() {
    let g = scen().battlefield(0, &["Swamp", "Lane A Totem"]).hand(0, &["Lane A Offering"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Lane A Offering", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "no artifact or creature to sacrifice");
    let mut g = scen().battlefield(0, &["Swamp", "Lane A Trinket", "Lane A Bear"]).hand(0, &["Lane A Offering"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Lane A Offering", Zone::Hand);
    cast(&mut g, c);
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    assert!(g.has_option(Choice::Card(bf(&g, 0, "Lane A Trinket"))) && g.has_option(Choice::Card(bf(&g, 0, "Lane A Bear"))));
    g.choose(Choice::Card(bf(&g, 0, "Lane A Trinket")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Lane A Trinket", Zone::Graveyard), 1);
    assert_eq!(g.player(0).hand.len(), 2);
}

/// "When this creature leaves the battlefield, put its counters on target
/// creature you control." (Broodguard Elite; its counters as it last existed,
/// CR 608.2h: every kind.)
#[test]
fn put_its_counters_on_target_creature_you_control_cr608_2h() {
    let mut g = scen()
        .battlefield(0, &["Lane A Donor", "Lane A Bear", "Mountain"])
        .counters(0, "Lane A Donor", Counter::P1P1, 3)
        .counters(0, "Lane A Donor", Counter::Kw(Kw::Flying), 1)
        .hand(0, &["Lane A Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lane A Bolt", Zone::Hand);
    let donor = bf(&g, 0, "Lane A Donor");
    cast(&mut g, bolt);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(donor));
    pay(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bf(g, 0, "Lane A Bear"))));
    let bear = bf(&g, 0, "Lane A Bear");
    assert_eq!(g.counters_on(bear, Counter::P1P1), 3, "{}", log(&g));
    assert!(g.has_kw(bear, Kw::Flying));
}
