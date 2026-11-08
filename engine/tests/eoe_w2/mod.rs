//! Shared fixtures of the Edge of Eternities W2 card tests (ENGINE.md
//! 99.12, mana and lands): the EoE card table plus the custom test cards
//! (`common`'s and W2's own, written as card IR so they go through the same
//! lowering), and the drivers the tests share.

#![allow(dead_code)]

use engine::db::{AbilitySlot, CardDb};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

#[path = "../common/mod.rs"]
mod common;

pub fn card(name: &str, printing: Value) -> Value {
    let mut p = printing;
    p["$t"] = json!("CardPrinting");
    p["name"] = json!(name);
    for (k, v) in [("mana_cost", json!({"$t": "ManaCost"})), ("subtypes", json!([])), ("mana_cost_str", json!("")), ("type_line", json!("")), ("supertypes", json!([]))] {
        if p.get(k).is_none() {
            p[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": p})
}

/// A mana ability: {T}: add `produces`, spending restriction `only`.
pub fn tap_mana(produces: Value, only: Option<&str>) -> Value {
    let mut e = json!({"$t": "AddManaToPool", "produces": produces});
    if let Some(r) = only {
        e["spend_restriction"] = json!(r);
    }
    json!({"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true}, "effect": e, "is_mana_ability": true})
}

/// "{n}: draw a card."
pub fn pay_draw(n: i64) -> Value {
    json!({"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": {"$t": "ManaCost", "generic": n}, "mana_str": format!("{{{n}}}")},
        "effect": {"$t": "DrawCards", "amount": 1}})
}

/// W2's custom test cards: the restrictions of cards whose IR another
/// workstream fixes (Sunken Citadel, Power Depot) and the purposes no EoE
/// card has together.
pub fn w2_cards() -> Vec<card_ir::Card> {
    let v = vec![
        // Sunken Citadel's restricted ability, of a fixed colour: "{T}: Add
        // {U}{U}. Spend this mana only to activate abilities of land
        // sources."
        card("Test Citadel", json!({"card_types": ["Land"], "activated_abilities": [tap_mana(json!([["U", 2]]), Some("land_activation"))]})),
        // A land with "{2}: Draw a card." and one with "{1}: Draw a card."
        // (and, as a land has one, a mana ability whose {C} pays neither:
        // for legendary spells only).
        card("Test Land Engine", json!({"card_types": ["Land"], "activated_abilities": [tap_mana(json!([["C", 1]]), Some("legendary_spell")), pay_draw(2)]})),
        card("Test Land Spark", json!({"card_types": ["Land"], "activated_abilities": [tap_mana(json!([["C", 1]]), Some("legendary_spell")), pay_draw(1)]})),
        // An artifact with "{1}: Draw a card."
        card("Test Artifact Engine", json!({"card_types": ["Artifact"], "mana_cost": {"$t": "ManaCost", "generic": 3}, "mana_cost_str": "{3}",
            "activated_abilities": [pay_draw(1)]})),
        // "{T}: Add {C}{C}. Spend this mana only to cast artifact spells."
        card("Test Workshop", json!({"card_types": ["Land"], "activated_abilities": [tap_mana(json!([["C", 2]]), Some("artifact_spell"))]})),
        // A colorless Eldrazi artifact creature ({1}) and a colorless
        // Eldrazi with "{1}: Draw a card."
        card("Test Eldrazi Relic", json!({"card_types": ["Artifact", "Creature"], "subtypes": ["Eldrazi"], "power": 1, "toughness": 1,
            "mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}"})),
        card("Test Eldrazi Seer", json!({"card_types": ["Creature"], "subtypes": ["Eldrazi"], "power": 1, "toughness": 1,
            "mana_cost": {"$t": "ManaCost", "generic": 2}, "mana_cost_str": "{2}", "activated_abilities": [pay_draw(1)]})),
        // A blue Eldrazi ({U}): not colorless.
        card("Test Blue Eldrazi", json!({"card_types": ["Creature"], "subtypes": ["Eldrazi"], "power": 1, "toughness": 1,
            "mana_cost": {"$t": "ManaCost", "colored": [["U", 1]]}, "mana_cost_str": "{U}"})),
    ];
    v.into_iter().map(|v| serde_json::from_value(v).expect("W2 test card IR")).collect()
}

/// The EoE card table (`CardDb::eoe`'s cards) plus the custom test cards,
/// lowered once per test binary.
pub fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_eoe_document().cards;
        let custom: Vec<card_ir::Card> = common::custom_cards().into_iter().chain(w2_cards()).collect();
        cards.extend(custom.iter().cloned());
        let db = CardDb::from_cards(&cards);
        for c in w2_cards() {
            db.expect_id(&c.name).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

pub fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

pub fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

pub fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

pub fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

pub fn opts(g: &Game) -> Vec<Choice> {
    g.pending_decision().map_or(Vec::new(), |d| d.options.iter().map(|o| o.choice).collect())
}

/// The first of `prefs` the pending decision offers.
pub fn pick(g: &Game, prefs: &[Choice]) -> Option<Choice> {
    prefs.iter().copied().find(|&c| g.has_option(c))
}

/// Answer decisions until `stop`: priority is passed, the rest answered by
/// `answer` (None: the first option).
pub fn run_until(g: &mut Game, stop: impl Fn(&Game) -> bool, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..2_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", g.log_lines().join("\n")));
        let c = match d.kind {
            DecisionKind::Priority => Choice::Pass,
            DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => answer(g).unwrap_or(Choice::Pass),
            _ => answer(g).unwrap_or(d.options[0].choice),
        };
        g.choose(c);
    }
    panic!("run_until: no progress\n{}", g.log_lines().join("\n"));
}

/// Run the stack down; `answer` answers what is asked (None: first option).
pub fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..500 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

pub fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Take action `c` (a cast or an activation), answer its choices with
/// `answer` (None: first option; payment by the first sources), until
/// priority returns.
pub fn start_with(g: &mut Game, c: Choice, answer: impl Fn(&Game) -> Option<Choice>) {
    g.choose(c);
    for _ in 0..200 {
        if at_priority(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let ch = answer(g).unwrap_or(d.options[0].choice);
        g.choose(ch);
    }
    panic!("start_with: no progress\n{}", g.log_lines().join("\n"));
}

/// Pay with sources on the named permanents (one pick each, `slot` of each),
/// in order, then finish the payment with first options.
pub fn pay_with(g: &mut Game, picks: &[Choice]) {
    for &c in picks {
        assert_eq!(g.kind(), Some(DecisionKind::PayMana), "{:?}\n{}", g.options_desc(), g.log_lines().join("\n"));
        assert!(g.has_option(c), "{c:?} not offered: {:?}", g.options_desc());
        g.choose(c);
    }
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
pub fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    run_until(g, stop, |_| None);
}

/// Until P0's (or P1's) next main phase 1 with an empty stack.
pub fn main1_of(p: PlayerId) -> impl Fn(&Game) -> bool {
    move |g: &Game| g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty()
}

/// A mana cost from its symbols ("{2}{U}").
pub fn cost(s: &str) -> engine::mana::ManaCost {
    engine::mana::ManaCost::parse(s).unwrap_or_else(|| panic!("cost {s}"))
}
