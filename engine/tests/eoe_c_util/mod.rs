//! Lane C of the Edge of Eternities port (ENGINE.md 99.4): the set's card
//! table plus custom test cards for lane C's features whose real cards also
//! need another lane's features, and the drive helpers the card tests share.

#![allow(dead_code)]

use engine::db::{AbilitySlot, CardDb};
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

fn cost(s: &str) -> Value {
    let mut generic = 0;
    let mut colored: Vec<(String, i64)> = Vec::new();
    for sym in s.trim_start_matches('{').trim_end_matches('}').split("}{").filter(|x| !x.is_empty()) {
        if let Ok(n) = sym.parse::<i64>() {
            generic += n;
        } else if let Some(e) = colored.iter_mut().find(|e| e.0 == sym) {
            e.1 += 1;
        } else {
            colored.push((sym.to_string(), 1));
        }
    }
    let mut m = json!({"$t": "ManaCost"});
    if generic > 0 {
        m["generic"] = json!(generic);
    }
    if !colored.is_empty() {
        m["colored"] = json!(colored);
    }
    m
}

/// A custom card printing: `types` its card types, `extra` merged in.
fn card(name: &str, mana: &str, types: &[&str], subtypes: &[&str], extra: Value) -> Value {
    let mut pr = json!({
        "$t": "CardPrinting",
        "name": name,
        "mana_cost_str": mana,
        "mana_cost": cost(mana),
        "type_line": types.join(" "),
        "supertypes": [],
        "card_types": types,
        "subtypes": subtypes,
    });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            pr[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": pr})
}

fn etb(effect: Value) -> Value {
    json!({"$t": "TriggeredAbility", "event": "enters", "predicate": {"$t": "IsSelf"}, "effect": effect})
}

/// Lane C's custom test cards.
pub fn custom_cards() -> Vec<card_ir::Card> {
    let creature = |name: &str, mana: &str, p: i64, t: i64, subtypes: &[&str], extra: Value| {
        let mut v = card(name, mana, &["Creature"], subtypes, extra);
        v["printing"]["power"] = json!(p);
        v["printing"]["toughness"] = json!(t);
        v
    };
    let cards = vec![
        creature("Test Bear", "{1}{G}", 2, 2, &["Bear"], json!({})),
        creature("Test Giant", "{4}{G}", 6, 6, &["Giant"], json!({})),
        // Chorale of the Void's void ability.
        card("Test Void Altar", "{1}{B}", &["Enchantment"], &[], json!({"triggered_abilities": [{"$t": "TriggeredAbility",
            "event": "beginning_of_end_step", "predicate": {"$t": "Controlled"}, "effect": {"$t": "SacrificeSelfUnlessVoidActive"}}]})),
        // Alpharael, Stonechosen's / Roving Actuator's intervening "if".
        creature("Test Void Raider", "{1}{B}", 2, 2, &["Rogue"], json!({"triggered_abilities": [{"$t": "TriggeredAbility",
            "event": "attacks", "predicate": {"$t": "And", "left": {"$t": "IsSelf"}, "right": {"$t": "StateBound", "inner": {"$t": "VoidActive"}}},
            "effect": {"$t": "DrawCards", "amount": 1}}]})),
        // Entropic Battlecruiser's attack trigger.
        creature("Test Tormentor", "{2}{B}", 2, 2, &["Horror"], json!({"triggered_abilities": [etb(json!({"$t": "EachOpponentDiscardsOrLosesLife"}))]})),
        // Specimen Freighter's attack trigger.
        creature("Test Miller", "{2}{U}", 1, 1, &["Wizard"], json!({"triggered_abilities": [etb(json!({"$t": "EachOpponentMillsCards", "amount": 4}))]})),
        // Infinite Guideline Station's attack trigger.
        creature("Test Prism", "{W}{U}", 1, 1, &["Wizard"], json!({"triggered_abilities": [etb(json!({"$t": "DrawCardPerMulticoloredPermanent"}))]})),
        // Hardlight Containment's enter trigger.
        card("Test Containment", "{1}{W}", &["Enchantment"], &[], json!({"triggered_abilities": [etb(json!({
            "$t": "ExileTargetCreatureUntilSourceLeaves", "opponents_only": true}))]})),
        // "Exile target creature or planeswalker" (Archenemy's Charm's first
        // mode with its full target filter).
        card("Test Exile", "{2}{B}", &["Instant"], &[], json!({"spell_effect": {"$t": "ExileTargetCreature", "targets": [{"$t": "TargetSpec",
            "filter": {"$t": "ObjectOr", "left": {"$t": "IsCreature"}, "right": {"$t": "IsPlaneswalker"}}}]}})),
        // Mental Modulation's cost reduction.
        card("Test Modulation", "{1}{U}", &["Instant"], &[], json!({"cost_reductions": [{"$t": "CostReduction", "amount": 1,
            "condition": {"$t": "DuringYourTurn"}}], "spell_effect": {"$t": "DrawCards", "amount": 1}})),
        // Starport Security's cost reduction (with a plain filter).
        card("Test Beacon", "{1}", &["Artifact"], &[], json!({"activated_abilities": [{"$t": "ActivatedAbility",
            "cost": {"$t": "ActivationCost", "mana": cost("{3}"), "mana_str": "{3}"},
            "cost_reductions": [{"$t": "CostReduction", "amount": 2, "condition": {"$t": "ControlPermanent", "filter": {"$t": "HasSubtype", "name": "Giant"}}}],
            "effect": {"$t": "DrawCards", "amount": 1}}]})),
        // Thaumaton Torpedo's cost reduction (with another effect).
        card("Test Torpedo", "{1}", &["Artifact"], &[], json!({"activated_abilities": [{"$t": "ActivatedAbility",
            "cost": {"$t": "ActivationCost", "mana": cost("{4}"), "mana_str": "{4}"},
            "cost_reductions": [{"$t": "CostReduction", "amount": 3, "condition": {"$t": "AttackedWithThisTurn", "filter": {"$t": "HasSubtype", "name": "Giant"}}}],
            "effect": {"$t": "DrawCards", "amount": 1}}]})),
        // A Spacecraft card (an artifact, not a creature).
        card("Test Spacecraft", "{3}", &["Artifact"], &["Spacecraft"], json!({})),
        // Mystifying Maze's ability (on an artifact).
        card("Test Maze", "{1}", &["Artifact"], &[], json!({"activated_abilities": [{"$t": "ActivatedAbility",
            "cost": {"$t": "ActivationCost", "mana": cost("{1}"), "mana_str": "{1}", "tap_self": true},
            "effect": {"$t": "ExileTargetAttackingCreatureUntilEndStep"}}]})),
        // Plaza of Heroes' "{3}, {T}, Exile this land:" cost (on an artifact).
        card("Test Escape Pod", "{1}", &["Artifact"], &[], json!({"activated_abilities": [{"$t": "ActivatedAbility",
            "cost": {"$t": "ActivationCost", "mana": cost("{1}"), "mana_str": "{1}", "tap_self": true, "exile_self": true},
            "effect": {"$t": "DrawCards", "amount": 1}}]})),
        // Sliver Overlord's search.
        card("Test Hive Seeker", "{1}", &["Artifact"], &[], json!({"activated_abilities": [{"$t": "ActivatedAbility",
            "cost": {"$t": "ActivationCost", "mana": cost("{1}"), "mana_str": "{1}"},
            "effect": {"$t": "SearchLibraryForCard", "filter_name": "sliver"}}]})),
        creature("Test Sliver", "{1}{G}", 1, 1, &["Sliver"], json!({})),
        // Broodguard Elite's "enters with X +1/+1 counters" and "Warp {X}{G}".
        {
            let mut v = creature("Test Warp Hydra", "{X}{G}{G}", 0, 0, &["Hydra"], json!({"enters_with_counters_from_x": "x",
                "warp_cost": {"$t": "ManaCost", "colored": [["G", 1]], "x_pips": 1}, "warp_cost_str": "{X}{G}"}));
            v["printing"]["mana_cost"] = json!({"$t": "ManaCost", "colored": [["G", 2]], "x_pips": 1});
            v
        },
        // Entropic Battlecruiser's "Whenever an opponent discards a card".
        card("Test Discard Watcher", "{1}", &["Artifact"], &[], json!({"triggered_abilities": [{"$t": "TriggeredAbility",
            "event": "discards_card", "predicate": {"$t": "Controlled", "by": "opponent"}, "effect": {"$t": "GainLife", "amount": 2}}]})),
    ];
    cards.into_iter().map(|v| serde_json::from_value(v).expect("custom card IR")).collect()
}

/// The Edge of Eternities table plus lane C's custom cards.
pub fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_eoe_document().cards;
        cards.extend(custom_cards());
        let db = CardDb::from_cards(&cards);
        for c in custom_cards() {
            db.expect_id(&c.name).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

pub fn scen() -> engine::testkit::Scenario {
    engine::testkit::Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Forest"; 12])
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

pub fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Pay with the first source offered until the payment is over.
pub fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
    }
}

/// Run the stack down; `answer` answers what is asked (None: first option).
pub fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
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

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
pub fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
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
    panic!("quiet_until: no progress\n{}", log(g));
}

/// Pass priority until attackers are declared, then attack with `names`.
pub fn attack_with(g: &mut Game, names: &[&str]) {
    for _ in 0..100 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || at_priority(g));
        if g.kind() == Some(DecisionKind::DeclareAttackers) {
            break;
        }
        g.pass();
    }
    for n in names {
        let c = bf(g, 0, n);
        g.choose(Choice::Card(c));
        g.drive_until(|g| g.kind() != Some(DecisionKind::AttackTarget));
    }
    g.choose(Choice::Pass);
}

/// Cast `name` from P`p`'s hand, choosing `targets` in order, and pay.
pub fn cast(g: &mut Game, p: PlayerId, name: &str, targets: &[Choice]) -> CardIdx {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    for &t in targets {
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(t);
    }
    pay(g);
    c
}

/// Untapped lands of `p`.
pub fn untapped(g: &Game, p: PlayerId) -> usize {
    g.lands_of(p).filter(|&l| !g.obj(l).tapped).count()
}

/// To P`p`'s end step with priority (the end step's triggers waiting or on
/// the stack).
pub fn to_end_step(g: &mut Game) {
    let t = g.turn().number;
    quiet_until(g, |g| g.turn().number == t && g.turn().step == Step::End && (at_priority(g) || g.kind() == Some(DecisionKind::OrderTriggers)));
}
