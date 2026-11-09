//! Shared fixtures of W6's card tests (ENGINE.md 99.16): the EoE card table
//! plus `common`'s custom cards and W6's own (card IR, through the same
//! lowering), and the drivers the tests share.

#![allow(dead_code)]

use engine::db::CardDb;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

#[path = "../common/mod.rs"]
mod common;

fn card(name: &str, printing: Value) -> Value {
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

/// W6's custom test cards: one feature each, in the IR's shapes.
pub fn w6_cards() -> Vec<card_ir::Card> {
    let v = vec![
        // "You may look at the top card of your library any time. / You may
        // cast artifact spells from the top of your library." (Mm'menon, the
        // Right Hand's middle abilities, alone.)
        card("W6 Top Caster", json!({"card_types": ["Creature"], "subtypes": ["Test"], "power": 1, "toughness": 1,
            "mana_cost": {"$t": "ManaCost", "colored": [["U", 1]]}, "mana_cost_str": "{U}",
            "grants_look_at_top_of_library": true, "grants_cast_artifact_from_top_of_library": true})),
        // "As this land enters, choose a color. / {T}: Add one mana of the
        // chosen color." (Sunken Citadel's first two lines.)
        card("W6 Chosen Land", json!({"card_types": ["Land"], "enters_choosing_color": true,
            "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true},
                "effect": {"$t": "AddManaToPool", "produces": [["chosen", 1]]}, "is_mana_ability": true}]})),
        // "Modular 2" on an artifact creature (CR 702.43a).
        card("W6 Modular Bot", json!({"card_types": ["Artifact", "Creature"], "subtypes": ["Construct"], "power": 0, "toughness": 0,
            "mana_cost": {"$t": "ManaCost", "generic": 2}, "mana_cost_str": "{2}",
            "keywords": [{"$t": "Keyword", "kind": "modular", "n": 2}]})),
        // "Modular 1" on a noncreature artifact (Power Depot's, a land
        // aside).
        card("W6 Modular Relic", json!({"card_types": ["Artifact"],
            "mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}",
            "keywords": [{"$t": "Keyword", "kind": "modular", "n": 1}]})),
    ];
    v.into_iter().map(|v| serde_json::from_value(v).expect("W6 test card IR")).collect()
}

/// The EoE card table plus the custom test cards, lowered once per test
/// binary.
pub fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_eoe_document().cards;
        cards.extend(common::custom_cards());
        cards.extend(w6_cards());
        let db = CardDb::from_cards(&cards);
        for c in w6_cards() {
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

pub fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

pub fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

pub fn pick(g: &Game, prefs: &[Choice]) -> Option<Choice> {
    prefs.iter().copied().find(|&c| g.has_option(c))
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
    panic!("resolve_with: no progress\n{}", log(g));
}

pub fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Take action `c`, answer its choices with `answer` (None: first option)
/// until priority returns.
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
    panic!("start_with: no progress");
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
pub fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..2_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress\n{}", log(g));
}

pub fn main1_of(p: PlayerId) -> impl Fn(&Game) -> bool {
    move |g: &Game| g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty()
}
