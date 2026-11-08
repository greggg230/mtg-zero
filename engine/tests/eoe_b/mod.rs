//! Shared fixtures of the Edge of Eternities lane B card tests (ENGINE.md
//! 99.3): the EoE card table plus the custom test cards (`common`'s and
//! lane B's own, written as card IR so they go through the same lowering),
//! and the drivers the tests share.

#![allow(dead_code)]

use engine::db::{AbilitySlot, CardDb};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

#[path = "../common/mod.rs"]
mod common;

/// The Lander token as the IR writes it once its {T} is restored
/// (docs/ir_defects.md 42): the shape the 13 `CreateToken` cards will have.
fn fixed_lander() -> Value {
    json!({"$t": "CardPrinting", "activated_abilities": [{"$t": "ActivatedAbility",
        "cost": {"$t": "ActivationCost", "mana": {"$t": "ManaCost", "generic": 2}, "mana_str": "{2}",
            "sacrifice_self": true, "tap_self": true},
        "effect": {"$t": "SearchLibraryForBasicLandTapped"}}],
        "card_types": ["Artifact"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "name": "Lander",
        "subtypes": ["Lander"], "supertypes": ["Token"], "type_line": "Token Artifact - Lander"})
}

fn card(name: &str, printing: Value) -> Value {
    let mut p = printing;
    p["$t"] = json!("CardPrinting");
    p["name"] = json!(name);
    if p.get("mana_cost").is_none() {
        p["mana_cost"] = json!({"$t": "ManaCost"});
    }
    if p.get("subtypes").is_none() {
        p["subtypes"] = json!([]);
    }
    if p.get("mana_cost_str").is_none() {
        p["mana_cost_str"] = json!("");
    }
    if p.get("type_line").is_none() {
        p["type_line"] = json!("");
    }
    if p.get("supertypes").is_none() {
        p["supertypes"] = json!([]);
    }
    json!({"name": name, "custom": true, "printing": p})
}

/// Lane B's custom test cards: the features of cards the IR defects block,
/// in the shapes the fixed IR will have.
pub fn lane_b_cards() -> Vec<card_ir::Card> {
    let lander = |pred: Value| {
        json!([{"$t": "TriggeredAbility", "event": "enters", "predicate": pred,
            "effect": {"$t": "CreateToken", "token_printing": fixed_lander()}}])
    };
    let v = vec![
        // "When this creature enters, create a Lander token." (Galactic
        // Wayfarer's shape.)
        card("Test Lander Maker", json!({"card_types": ["Creature"], "subtypes": ["Test"], "power": 1, "toughness": 1,
            "mana_cost": {"$t": "ManaCost", "generic": 1, "colored": [["G", 1]]}, "mana_cost_str": "{1}{G}",
            "triggered_abilities": lander(json!({"$t": "IsSelf"}))})),
        // "When this creature enters, if an opponent controls more lands than
        // you, create a Lander token." (Sunstar Expansionist's.)
        card("Test Expansionist", json!({"card_types": ["Creature"], "subtypes": ["Test"], "power": 2, "toughness": 3,
            "mana_cost": {"$t": "ManaCost", "generic": 1, "colored": [["W", 1]]}, "mana_cost_str": "{1}{W}",
            "triggered_abilities": lander(json!({"$t": "And", "left": {"$t": "IsSelf"},
                "right": {"$t": "StateBound", "inner": {"$t": "OpponentControlsMoreLandsThanYou"}}}))})),
        // "{T}: Add {C}. / {1}, {T}: Untap target land." (Deserted Temple's,
        // its {C} restored: docs/ir_defects.md 40.)
        card("Test Temple", json!({"card_types": ["Land"], "produces": ["C"], "mana_only_from_produces": true,
            "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost",
                "mana": {"$t": "ManaCost", "generic": 1}, "mana_str": "{1}", "tap_self": true},
                "effect": {"$t": "UntapTargetLand"}}]})),
        // "{T}: Add {C}. / {2}, {T}: You may put a basic land card from your
        // hand onto the battlefield tapped." (Terrain Generator's.)
        card("Test Generator", json!({"card_types": ["Land"], "produces": ["C"], "mana_only_from_produces": true,
            "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost",
                "mana": {"$t": "ManaCost", "generic": 2}, "mana_str": "{2}", "tap_self": true},
                "effect": {"$t": "PutBasicLandFromHandOntoBattlefieldTapped"}}]})),
        // "{6}, {T}, Sacrifice this artifact: Destroy target nonland
        // permanent." (Thaumaton Torpedo's ability, without its cost
        // reduction: lane C's.)
        card("Test Torpedo", json!({"card_types": ["Artifact"],
            "mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}",
            "activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost",
                "mana": {"$t": "ManaCost", "generic": 6}, "mana_str": "{6}", "sacrifice_self": true, "tap_self": true},
                "effect": {"$t": "DestroyTargetNonlandPermanent", "targets": [{"$t": "TargetSpec", "filter": {"$t": "ObjectAnd",
                    "left": {"$t": "ObjectNot", "inner": {"$t": "IsLand"}},
                    "right": {"$t": "ObjectNot", "inner": {"$t": "IsPlayer"}}}}]}}]})),
    ];
    v.into_iter().map(|v| serde_json::from_value(v).expect("lane B test card IR")).collect()
}

/// The EoE card table (`CardDb::eoe`'s cards) plus the custom test cards,
/// lowered once per test binary.
pub fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_eoe_document().cards;
        let custom: Vec<card_ir::Card> = common::custom_cards().into_iter().chain(lane_b_cards()).collect();
        cards.extend(custom.iter().cloned());
        let db = CardDb::from_cards(&cards);
        for c in lane_b_cards() {
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

pub fn def(name: &str) -> u16 {
    db().id(name).unwrap_or_else(|| panic!("{name}: not supported in the EoE table"))
}

pub fn subtype(g: &Game, c: CardIdx, s: &str) -> bool {
    db().subtype_id(s).is_some_and(|id| g.has_subtype(c, id))
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
    panic!("start_with: no progress");
}

/// Cast `name` from `p`'s hand, then run the stack down; `answer` answers
/// every choice (targets, modes, mays).
pub fn cast_and_resolve(g: &mut Game, p: PlayerId, name: &str, answer: impl Fn(&Game) -> Option<Choice> + Copy) {
    let c = g.card(p, name, Zone::Hand);
    start_with(g, Choice::Cast(c), answer);
    resolve_with(g, answer);
}

/// Play land `name` from `p`'s hand (a special action, CR 305.1), then run
/// any triggers down.
pub fn play_land(g: &mut Game, p: PlayerId, name: &str, answer: impl Fn(&Game) -> Option<Choice> + Copy) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Play(c));
    resolve_with(g, answer);
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
pub fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    run_until(g, stop, |_| None);
}

/// Until P0's (or P1's) next main phase 1 with an empty stack.
pub fn main1_of(p: PlayerId) -> impl Fn(&Game) -> bool {
    move |g: &Game| g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty()
}

pub fn count_named(g: &Game, p: PlayerId, name: &str) -> usize {
    g.permanents_of(p).filter(|&c| g.name(c) == name).count()
}
