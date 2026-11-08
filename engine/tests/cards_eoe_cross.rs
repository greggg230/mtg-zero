//! Edge of Eternities cards that need features of more than one lane
//! (ENGINE.md 99.6): finished after the lanes' branches merge.

use std::sync::OnceLock;

use engine::db::{AbilitySlot, CardDb};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
use serde_json::json;

/// The EoE table plus one custom card: an artifact creature Spacecraft
/// (a Spacecraft that can attack without Station, which is lane A's).
fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_eoe_document().cards;
        let ship = json!({"name": "Test Starship", "custom": true, "printing": {
            "$t": "CardPrinting", "name": "Test Starship", "mana_cost_str": "{3}",
            "mana_cost": {"$t": "ManaCost", "generic": 3}, "type_line": "Artifact Creature - Spacecraft",
            "supertypes": [], "card_types": ["Artifact", "Creature"], "subtypes": ["Spacecraft"],
            "power": 3, "toughness": 3}});
        cards.push(serde_json::from_value(ship).expect("custom card IR"));
        let db = CardDb::from_cards(&cards);
        db.expect_id("Test Starship").unwrap_or_else(|e| panic!("{e}"));
        db.expect_id("Thaumaton Torpedo").unwrap_or_else(|e| panic!("{e}"));
        db.leak()
    })
}

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Forest"; 12])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn torpedo(g: &Game) -> Choice {
    Choice::Ability(bf(g, 0, "Thaumaton Torpedo"), AbilitySlot::Activated(0))
}

/// Pass (declining attacks and blocks) or take the first option until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress\n{}", g.log_lines().join("\n"));
}

/// Pay what is asked, then let the stack resolve (first option for anything else).
fn pay_and_resolve(g: &mut Game) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    panic!("pay_and_resolve: no progress\n{}", g.log_lines().join("\n"));
}

/// Thaumaton Torpedo: "{6}, {T}, Sacrifice this artifact: Destroy target
/// nonland permanent" (CR 701.8a, 602.2): a nonland permanent, not a land or
/// a player; the Torpedo goes to the graveyard as a cost.
#[test]
fn thaumaton_torpedo_destroys_target_nonland_permanent_cr701_8a() {
    let mut g = scen()
        .battlefield(0, &["Thaumaton Torpedo"])
        .battlefield(0, &["Island"; 6])
        .battlefield(1, &["Test Starship", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let (ship, forest) = (bf(&g, 1, "Test Starship"), bf(&g, 1, "Forest"));
    let t = bf(&g, 0, "Thaumaton Torpedo");
    assert!(g.has_option(torpedo(&g)), "{{6}} with six Islands: {:?}", g.options_desc());
    g.choose(torpedo(&g));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(forest)), "not a land");
    assert!(!g.has_option(Choice::Target(Target::Player(1))), "not a player");
    let ts = g.tgt(ship);
    g.choose(ts);
    pay_and_resolve(&mut g);
    assert_eq!(g.obj(ship).zone, Zone::Graveyard, "destroyed\n{}", g.log_lines().join("\n"));
    assert_eq!(g.obj(t).zone, Zone::Graveyard, "sacrificed as a cost");
    assert_eq!(g.obj(forest).zone, Zone::Battlefield);
}

/// "This ability costs {3} less to activate if you attacked with a Spacecraft
/// this turn" (CR 602.2b, 601.2f): {3} is not enough before the attack, and
/// is after attacking with a Spacecraft.
#[test]
fn thaumaton_torpedo_costs_3_less_after_attacking_with_a_spacecraft_cr602_2b() {
    let mut g = scen()
        .battlefield(0, &["Thaumaton Torpedo", "Test Starship"])
        .battlefield(0, &["Island"; 3])
        .battlefield(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(torpedo(&g)), "{{6}} with three Islands");
    for _ in 0..100 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || at_priority(g));
        if g.kind() == Some(DecisionKind::DeclareAttackers) {
            break;
        }
        g.pass();
    }
    let s = bf(&g, 0, "Test Starship");
    g.choose(Choice::Card(s));
    g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.has_option(torpedo(&g)), "{{3}} after attacking with a Spacecraft: {:?}", g.options_desc());
}
