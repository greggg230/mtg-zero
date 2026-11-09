//! W6's mana and land pieces (ENGINE.md 99.16): modular (CR 702.43, Power
//! Depot's) on custom test cards.

mod eoe_w6_kit;

use engine::db::Counter;
use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_w6_kit::*;

/// P0 destroys its own artifact `c` with Shattered Wings; the modular
/// trigger targets `to` and its "may" is answered `yes`.
fn destroy(g: &mut Game, c: engine::CardIdx, to: engine::CardIdx, yes: bool) {
    let w = g.card(0, "Shattered Wings", Zone::Hand);
    start_with(g, Choice::Cast(w), |g| pick(g, &[g.tgt(c)]));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => pick(g, &[g.tgt(to)]),
        Some(DecisionKind::YesNo) => Some(if yes { Choice::Yes } else { Choice::Pass }),
        _ => None,
    });
}

/// Modular (CR 702.43a): "This permanent enters with two +1/+1 counters on
/// it. When it is put into a graveyard from the battlefield, you may put a
/// +1/+1 counter on target artifact creature for each +1/+1 counter on it"
/// (as it last existed, 608.2h). Declined: nothing.
#[test]
fn cr_702_43a_modular_enters_with_counters_and_passes_them_on() {
    for yes in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Forest", "Dockworker Drone"])
            .hand(0, &["W6 Modular Bot", "Shattered Wings"])
            .at(Step::Main1, 0)
            .start();
        let bot = g.card(0, "W6 Modular Bot", Zone::Hand);
        start_with(&mut g, Choice::Cast(bot), |_| None);
        settle(&mut g);
        assert_eq!(g.counters_on(bot, Counter::P1P1), 2, "{}", log(&g));
        let drone = bf(&g, 0, "Dockworker Drone");
        let before = g.counters_on(drone, Counter::P1P1);
        destroy(&mut g, bot, drone, yes);
        assert_eq!(g.obj(bot).zone, Zone::Graveyard);
        assert_eq!(g.counters_on(drone, Counter::P1P1), before + if yes { 2 } else { 0 }, "{}", log(&g));
    }
}

/// "When it's put into the graveyard from the battlefield, you may put its
/// +1/+1 counters onto target artifact creature, no matter what Power
/// Depot's card types are at the time" (Power Depot's ruling): a
/// noncreature artifact with modular 1 enters with a counter (it does
/// nothing there) and passes it on.
#[test]
fn cr_702_43a_modular_on_a_noncreature_artifact() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Dockworker Drone"])
        .hand(0, &["W6 Modular Relic", "Shattered Wings"])
        .at(Step::Main1, 0)
        .start();
    let relic = g.card(0, "W6 Modular Relic", Zone::Hand);
    start_with(&mut g, Choice::Cast(relic), |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(relic, Counter::P1P1), 1);
    let drone = bf(&g, 0, "Dockworker Drone");
    let before = g.counters_on(drone, Counter::P1P1);
    destroy(&mut g, relic, drone, true);
    assert_eq!(g.counters_on(drone, Counter::P1P1), before + 1, "{}", log(&g));
}

// ---- Power Depot (docs/ir_defects.md 40, 47) ----------------------------------

/// Power Depot: "This land enters tapped. / {T}: Add {C}. / {T}: Add one mana
/// of any color. Spend this mana only to cast artifact spells or activate
/// abilities of artifacts. / Modular 1." Played: tapped, with a +1/+1
/// counter ("which does nothing while it's a land", the ruling). Its
/// coloured mana casts an artifact spell with a coloured pip (Dockworker
/// Drone, {1}{W}, with a Plains... here with the Depot alone for {W}) but
/// not a nonartifact one (Honor, {W}).
#[test]
fn cr_106_6_power_depot_enters_tapped_with_modular_and_its_restricted_mana() {
    let mut g = scen().hand(0, &["Power Depot"]).at(Step::Main1, 0).start();
    let d = g.card(0, "Power Depot", Zone::Hand);
    g.choose(Choice::Play(d));
    settle(&mut g);
    assert!(g.obj(d).tapped, "enters tapped");
    assert_eq!(g.counters_on(d, Counter::P1P1), 1, "modular 1");
    let g = scen().battlefield(0, &["Power Depot", "Plains"]).hand(0, &["Dockworker Drone", "Honor"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let drone = g.card(0, "Dockworker Drone", Zone::Hand);
    assert!(g.has_option(Choice::Cast(drone)), "an artifact spell: {:?}", g.options_desc());
    let g = scen().battlefield(0, &["Power Depot"]).hand(0, &["Honor"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let honor = g.card(0, "Honor", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(honor)), "not a nonartifact spell: {:?}", g.options_desc());
}

/// Power Depot's modular (CR 702.43a; the ruling: "no matter what Power
/// Depot's card types are at the time"): destroyed (Shattered Wings, target
/// artifact), its counter goes onto target artifact creature.
#[test]
fn cr_702_43a_power_depot_passes_its_counter_on() {
    let mut g = scen()
        .battlefield(0, &["Power Depot", "Forest", "Forest", "Forest", "Dockworker Drone"])
        .counters(0, "Power Depot", Counter::P1P1, 1)
        .hand(0, &["Shattered Wings"])
        .at(Step::Main1, 0)
        .start();
    let depot = bf(&g, 0, "Power Depot");
    let drone = bf(&g, 0, "Dockworker Drone");
    let before = g.counters_on(drone, Counter::P1P1);
    destroy(&mut g, depot, drone, true);
    assert_eq!(g.obj(depot).zone, Zone::Graveyard);
    assert_eq!(g.counters_on(drone, Counter::P1P1), before + 1, "{}", log(&g));
}

// ---- Contested War Zone (docs/ir_defects.md 161) --------------------------------

/// P1 attacks with Test Bear; P0 blocks with `blocker` if given.
fn p1_attacks(g: &mut Game, blocker: Option<engine::CardIdx>) {
    let bear = bf(g, 1, "Test Bear");
    let mut attacked = false;
    for _ in 0..400 {
        if attacked && g.turn().step == Step::Main2 && at_priority(g) && g.stack().is_empty() {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        let c = match d.kind {
            DecisionKind::DeclareAttackers if d.player == 1 && g.has_option(Choice::Card(bear)) => {
                attacked = true;
                Choice::Card(bear)
            }
            DecisionKind::DeclareBlockers => match blocker {
                Some(b) if g.has_option(Choice::Card(bear)) && g.pending_decision().unwrap().player == 0 => {
                    let _ = b;
                    Choice::Card(bear)
                }
                _ => Choice::Pass,
            },
            DecisionKind::Priority | DecisionKind::DeclareAttackers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("p1_attacks: no progress\n{}", log(g));
}

/// Contested War Zone: "Whenever a creature deals combat damage to you, that
/// creature's controller gains control of this land." Unblocked, P1's Test
/// Bear damages P0: P1 controls the War Zone (it doesn't untap or tap, the
/// ruling). Blocked by P0's creature, the damage is dealt to the creature,
/// not to P0: no change (docs/ir_defects.md 161).
#[test]
fn cr_510_2_contested_war_zone_goes_to_the_damaging_creatures_controller() {
    for block in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Contested War Zone", "Test Bear"])
            .battlefield(1, &["Test Bear"])
            .at(Step::Main1, 1)
            .start();
        let zone = bf(&g, 0, "Contested War Zone");
        let ogre = bf(&g, 0, "Test Bear");
        p1_attacks(&mut g, block.then_some(ogre));
        assert_eq!(g.obj(zone).controller, if block { 0 } else { 1 }, "blocked {block}\n{}", log(&g));
        assert!(!g.obj(zone).tapped);
    }
}
