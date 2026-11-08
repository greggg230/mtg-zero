//! Edge of Eternities W2 (ENGINE.md 99.12): the lands and the other cards
//! (the spending restrictions are in `cards_eoe_w2.rs`).

mod eoe_w2;

use eoe_w2::*;
use engine::db::{Counter, Kw};
use engine::{Choice, DecisionKind, Step, Zone};

/// Declare `c` (the active player's) an attacker of the opponent and finish
/// the declaration.
fn attack(g: &mut engine::Game, c: engine::CardIdx) {
    run_until(g, |g| g.kind() == Some(DecisionKind::DeclareAttackers), |_| None);
    g.choose(Choice::Card(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
}

fn charge() -> Counter {
    Counter::Named(db().subtype_id("counter:charge").expect("charge counters"))
}

/// The Eternity Elevator: "{T}: Add {C}{C}{C}." and, with 20 or more charge
/// counters (station, CR 702.184, 722.2a), "{T}: Add X mana of any one color,
/// where X is the number of charge counters on The Eternity Elevator" (CR
/// 106.1a).
#[test]
fn cr_106_1a_the_eternity_elevator_adds_x_mana_of_one_color_at_20() {
    let g = scen().battlefield(0, &["The Eternity Elevator"]).counters(0, "The Eternity Elevator", charge(), 19).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{3}")) && !g.can_pay(0, &cost("{4}")), "19 counters: {{C}}{{C}}{{C}} only");
    assert!(!g.can_pay(0, &cost("{G}")));
    let g = scen().battlefield(0, &["The Eternity Elevator"]).counters(0, "The Eternity Elevator", charge(), 21).at(Step::Main1, 0).start();
    let greens = "{G}".repeat(21);
    assert!(g.can_pay(0, &cost(&greens)), "21 green mana");
    assert!(!g.can_pay(0, &cost(&format!("{greens}{{G}}"))));
    assert!(!g.can_pay(0, &cost("{G}{U}")), "of one color");
    assert!(g.can_pay(0, &cost("{U}{U}{U}{U}")));
}

/// Cascading Cataracts: indestructible; "{5}, {T}: Add five mana in any
/// combination of colors" (a mana ability with a mana cost, paid within the
/// payment it helps: CR 605.3a, 602.2b; its own mana never pays it).
#[test]
fn cr_605_3a_cascading_cataracts_turns_five_mana_into_five_of_any_colors() {
    let g = scen().battlefield(0, &["Cascading Cataracts", "Island", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    let cat = bf(&g, 0, "Cascading Cataracts");
    assert!(g.keywords_of(cat).has(Kw::Indestructible));
    assert!(g.can_pay(0, &cost("{W}{U}{B}{R}{G}")), "the Islands pay {{5}}, the Cataracts make the colours");
    assert!(g.can_pay(0, &cost("{R}{R}{G}{G}{W}")));
    assert!(!g.can_pay(0, &cost("{R}{R}{G}{G}{W}{U}")), "five, not six");
    let g = scen().battlefield(0, &["Cascading Cataracts", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{R}")), "four Islands can't pay {{5}}");
    assert!(g.can_pay(0, &cost("{5}")), "{{C}} and the Islands");
}

/// The Cataracts in a payment: picked, its {5} is paid by the payment's other
/// sources; a spell of {2}{R} (CR 601.2g-h, 605.3a).
#[test]
fn cr_601_2g_cascading_cataracts_pays_within_a_payment() {
    let mut g = scen()
        .battlefield(0, &["Cascading Cataracts", "Island", "Island", "Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["Test Double"])
        .at(Step::Main1, 0)
        .start();
    let cat = bf(&g, 0, "Cascading Cataracts");
    let spell = g.card(0, "Test Double", Zone::Hand);
    assert!(g.has_option(Choice::Cast(spell)));
    g.choose(Choice::Cast(spell));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    assert!(g.has_option(act(cat, 1)), "{:?}", g.options_desc());
    g.choose(act(cat, 1));
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(spell).zone, Zone::Battlefield);
    assert!(g.obj(cat).tapped);
    // (The Islands pay its {5}; its five mana pay {2}{R}, two stay in the
    // pool: the payment spends the virtual pool's mana first, ENGINE.md 18.)
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Island" && g.obj(c).tapped).count(), 5, "its {{5}}");
    assert_eq!(g.player(0).pool.total(), 2);
}

/// Crystal Quarry: "{5}, {T}: Add {W}{U}{B}{R}{G}" (CR 605.3a).
#[test]
fn cr_605_3a_crystal_quarry_makes_one_of_each_color() {
    let g = scen().battlefield(0, &["Crystal Quarry", "Island", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}{U}{B}{R}{G}")));
    assert!(!g.can_pay(0, &cost("{R}{R}")), "one of each");
    let g = scen().battlefield(0, &["Crystal Quarry", "Island", "Island", "Island", "Island"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{W}")));
}

/// Blast Zone: "This land enters with a charge counter on it." (CR 122.6)
/// and "{X}{X}, {T}: Put X charge counters on this land." (CR 107.3: X paid
/// twice).
#[test]
fn cr_107_3_blast_zone_enters_with_a_counter_and_takes_x_for_x_x() {
    let mut g = scen().battlefield(0, &["Island", "Island", "Island", "Island", "Island"]).hand(0, &["Blast Zone"]).at(Step::Main1, 0).start();
    let bz = g.card(0, "Blast Zone", Zone::Hand);
    g.choose(Choice::Play(bz));
    settle(&mut g);
    assert_eq!(g.counters_on(bz, charge()), 1);
    // ({T}: the land just played can be tapped: lands have no summoning
    // sickness, CR 302.6.) Five Islands: X is at most 2.
    g.choose(act(bz, 1));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "X: {:?}", g.options_desc());
    assert_eq!(opts(&g).len(), 3, "X = 0, 1 or 2");
    let x2 = opts(&g)[2];
    g.choose(x2);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(bz, charge()), 3);
    assert_eq!(g.permanents_of(0).filter(|&c| g.name(c) == "Island" && g.obj(c).tapped).count(), 4, "{{X}}{{X}} with X = 2");
}

/// "{3}, {T}, Sacrifice this land: Destroy each nonland permanent with mana
/// value equal to the number of charge counters on this land" (sacrificed as
/// the cost: its counters as it last existed, CR 608.2h; 701.8).
#[test]
fn cr_608_2h_blast_zone_destroys_by_its_last_known_counters() {
    let mut g = scen()
        .battlefield(0, &["Blast Zone", "Island", "Island", "Island", "Test Bear"])
        .battlefield(1, &["Test Flyer", "Test Double", "Forest"])
        .counters(0, "Blast Zone", charge(), 2)
        .at(Step::Main1, 0)
        .start();
    let bz = bf(&g, 0, "Blast Zone");
    g.choose(act(bz, 2));
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(bz).zone, Zone::Graveyard);
    assert!(g.find_card(0, "Test Bear", Zone::Graveyard).is_some(), "the Bear ({{1}}{{G}})");
    assert!(g.find_card(1, "Test Flyer", Zone::Graveyard).is_some(), "the Flyer ({{1}}{{U}})");
    assert!(g.find_card(1, "Test Double", Zone::Battlefield).is_some(), "mana value 3 stays");
    assert!(g.find_card(1, "Forest", Zone::Battlefield).is_some(), "lands stay");
}

/// Contested War Zone (its trigger as the Oracle text says it,
/// docs/ir_defects.md 161): "Whenever a creature deals combat damage to you,
/// that creature's controller gains control of this land" (CR 510.1, 603.2,
/// 613.1b).
#[test]
fn cr_613_1b_contested_war_zone_goes_to_the_attacker_that_damaged_you() {
    let mut g = scen().battlefield(0, &["Test War Zone"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 1).start();
    let zone = bf(&g, 0, "Test War Zone");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, bear);
    run_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g), |_| None);
    assert_eq!(g.player(0).life, 18);
    assert_eq!(g.obj(zone).controller, 1, "the Bear's controller gains control of it");
}

/// Combat damage to a creature you control is not combat damage to you: the
/// War Zone stays (CR 510.1c).
#[test]
fn cr_510_1c_contested_war_zone_ignores_damage_to_your_creatures() {
    let mut g = scen().battlefield(0, &["Test War Zone", "Test Reacher"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 1).start();
    let zone = bf(&g, 0, "Test War Zone");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, bear);
    run_until(
        &mut g,
        |g| g.turn().step == Step::Main2 && at_priority(g),
        |g| if g.kind() == Some(DecisionKind::DeclareBlockers) { pick(g, &[Choice::Card(bear)]) } else { None },
    );
    assert_eq!(g.player(0).life, 20, "blocked");
    assert_eq!(g.obj(zone).controller, 0);
}

/// "{1}, {T}: Attacking creatures get +1/+0 until end of turn": every
/// attacking creature, whoever controls it (CR 611.2c), activated by the
/// defending player too.
#[test]
fn cr_611_2c_contested_war_zone_pumps_every_attacking_creature() {
    let mut g = scen().battlefield(0, &["Test War Zone", "Island"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 1).start();
    let zone = bf(&g, 0, "Test War Zone");
    let bear = bf(&g, 1, "Test Bear");
    attack(&mut g, bear);
    run_until(&mut g, |g| at_priority(g) && g.pending_decision().unwrap().player == 0 && g.turn().step == Step::DeclareAttackers, |_| None);
    assert!(g.has_option(act(zone, 1)), "{:?}", g.options_desc());
    start_with(&mut g, act(zone, 1), |_| None);
    settle(&mut g);
    assert_eq!(g.power(bear), 3, "the opponent's attacker gets +1/+0");
}

/// Plaza of Heroes (its third ability as the Oracle text says it,
/// docs/ir_defects.md 160): "{T}: Add one mana of any color. Spend this mana
/// only to cast a legendary spell." (CR 106.6) and "{T}: Add one mana of any
/// color among legendary permanents you control." (CR 106.1, 105.4).
#[test]
fn cr_106_6_plaza_of_heroes_mana_for_legends_and_legends_colors() {
    let g = scen().battlefield(0, &["Test Plaza"]).hand(0, &["Test Legend", "Test Bear"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{W}")), "no legendary permanent: no colour but for a legendary spell");
    assert!(g.can_pay(0, &cost("{1}")));
    let g = scen().battlefield(0, &["Test Plaza", "Plains"]).hand(0, &["Test Legend", "Test Bear"]).at(Step::Main1, 0).start();
    let legend = g.card(0, "Test Legend", Zone::Hand);
    let bear = g.card(0, "Test Bear", Zone::Hand);
    assert!(g.has_option(Choice::Cast(legend)), "{{1}}{{W}}: the Plaza's restricted mana and the Plains");
    assert!(!g.has_option(Choice::Cast(bear)), "{{1}}{{G}}: no green");
    let g = scen().battlefield(0, &["Test Plaza", "Test Legend"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}")), "white among legendary permanents");
    assert!(!g.can_pay(0, &cost("{U}")));
}

/// "{3}, {T}, Exile this land: Target legendary creature gains hexproof and
/// indestructible until end of turn" (CR 702.11, 702.12).
#[test]
fn cr_702_12_plaza_of_heroes_protects_a_legendary_creature() {
    let mut g = scen().battlefield(0, &["Test Plaza", "Island", "Island", "Island", "Test Legend", "Test Bear"]).at(Step::Main1, 0).start();
    let plaza = bf(&g, 0, "Test Plaza");
    let legend = bf(&g, 0, "Test Legend");
    g.choose(act(plaza, 3));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(legend)) && !g.has_option(g.tgt(bf(&g, 0, "Test Bear"))), "legendary creatures only");
    let t = g.tgt(legend);
    g.choose(t);
    run_until(&mut g, at_priority, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(plaza).zone, Zone::Exile);
    assert!(g.keywords_of(legend).has(Kw::Hexproof) && g.keywords_of(legend).has(Kw::Indestructible));
}
