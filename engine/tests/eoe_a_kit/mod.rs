//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): the test card
//! table (the set's cards plus custom IR test cards, so a rule is tested on
//! its own; never in the fuzz or digest pools) and driving helpers.

#![allow(dead_code)]

use engine::db::{AbilitySlot, CardDb, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

pub fn cost(s: &str) -> Value {
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

fn merge(pr: &mut Value, extra: Value) {
    if let Value::Object(m) = extra {
        for (k, v) in m {
            pr[k] = v;
        }
    }
}

/// A custom card in IR form: `types` its card types, `subtypes` its
/// subtypes, `extra` more printing fields.
pub fn card(name: &str, mana: &str, types: &[&str], subtypes: &[&str], extra: Value) -> Value {
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
    merge(&mut pr, extra);
    json!({"name": name, "custom": true, "printing": pr})
}

pub fn creature(name: &str, mana: &str, p: i64, t: i64, extra: Value) -> Value {
    let mut e = json!({"power": p, "toughness": t});
    merge(&mut e, extra);
    card(name, mana, &["Creature"], &["Test"], e)
}

pub fn kw(k: &str) -> Value {
    json!({"$t": "Keyword", "kind": k})
}

fn target_creature() -> Value {
    json!([{"$t": "TargetSpec", "filter": {"$t": "IsCreature"}}])
}

/// The custom test cards of lane A.
pub fn custom_cards() -> Vec<card_ir::Card> {
    let none = json!({});
    let cards = vec![
        creature("Lane A Bear", "{1}{G}", 2, 2, none.clone()),
        creature("Lane A Giant", "{4}{G}", 5, 5, none.clone()),
        creature("Lane A Weakling", "{B}", -1, 3, none.clone()),
        card("Lane A Totem", "{1}", &["Enchantment"], &[], none.clone()),
        card("Lane A Trinket", "{1}", &["Artifact"], &[], none.clone()),
        creature("Lane A Robot", "{2}", 2, 2, json!({"card_types": ["Artifact", "Creature"], "subtypes": ["Robot"]})),
        // Lane A's effects that cross-lane cards use (ENGINE.md 99.2), each
        // on a card of its own: "Tap target artifact or creature."
        card("Lane A Tap", "{U}", &["Instant"], &[], json!({"spell_effect": {"$t": "TapTargetArtifactOrCreature",
            "targets": [{"$t": "TargetSpec", "filter": {"$t": "ObjectOr", "left": {"$t": "IsArtifact"}, "right": {"$t": "IsCreature"}}}]}})),
        // "Target artifact or creature gains indestructible until end of turn."
        card("Lane A Shield", "{W}", &["Instant"], &[], json!({"spell_effect": {"$t": "PumpTargetArtifactOrCreatureUntilEOT",
            "granted_keywords": [kw("indestructible")],
            "targets": [{"$t": "TargetSpec", "filter": {"$t": "ObjectOr", "left": {"$t": "IsArtifact"}, "right": {"$t": "IsCreature"}}}]}})),
        // "Put two +1/+1 counters on target creature you control. It gains
        // lifelink until end of turn."
        card("Lane A Charm", "{B}", &["Instant"], &[], json!({"spell_effect": {"$t": "PutPlusOneCountersAndGrantKeywordsUEOTOnTargetCreatureYouControl",
            "granted_keywords": [kw("lifelink")]}})),
        // "As an additional cost to cast this spell, sacrifice an artifact or
        // creature. / Draw two cards."
        card("Lane A Offering", "{B}", &["Sorcery"], &[], json!({"additional_cost_sacrifice_creature_or_artifact": true,
            "spell_effect": {"$t": "DrawCards", "amount": 2}})),
        // "When this creature leaves the battlefield, put its counters on
        // target creature you control."
        creature("Lane A Donor", "{1}{G}", 1, 1, json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "leaves_battlefield",
            "predicate": {"$t": "IsSelf"}, "effect": {"$t": "MoveSelfCountersToTargetCreatureYouControl"}}]})),
        // "Enchant artifact you control / Enchanted permanent has ward {1}."
        // (Hardlight Containment's ward; CR 702.21a.)
        card("Lane A Ward Aura", "{W}", &["Enchantment"], &["Aura"], json!({"aura_enchant_type": "artifact", "aura_yours_only": true,
            "attached_grants_ward_cost": cost("{1}"), "attached_grants_ward_str": "{1}"})),
        // "{1}, Sacrifice this creature: Search your library for an
        // Equipment card, put it onto the battlefield, then shuffle."
        creature(
            "Lane A Quartermaster",
            "{W}",
            1,
            1,
            json!({"activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "mana": cost("{1}"), "mana_str": "{1}", "sacrifice_self": true},
                "effect": {"$t": "SearchLibraryForEquipmentToBattlefield"}}]}),
        ),
        // "{T}: Add {G}."
        creature(
            "Lane A Dork",
            "{G}",
            1,
            1,
            json!({"activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true},
                "effect": {"$t": "AddManaToPool", "produces": [["G", 1]]}, "is_mana_ability": true}]}),
        ),
        // "Target creature gets +3/+3 until end of turn."
        card(
            "Lane A Growth",
            "{G}",
            &["Instant"],
            &[],
            json!({"spell_effect": {"$t": "PumpTargetCreatureUntilEOT", "power_bonus": 3, "toughness_bonus": 3}}),
        ),
        // "Lane A Bolt deals 10 damage to target creature."
        card(
            "Lane A Bolt",
            "{R}",
            &["Instant"],
            &[],
            json!({"spell_effect": {"$t": "DealDamageToTargetCreature", "amount": 10, "targets": target_creature()}}),
        ),
        // A Spacecraft (CR 722.2b) whose "2+" striation has a triggered
        // ability: "Station. / 2+ | At the beginning of combat on your turn,
        // you gain 2 life. / 4+ | Flying [3/3]".
        card(
            "Lane A Tier Ship",
            "{2}",
            &["Artifact"],
            &["Spacecraft"],
            json!({"power": 3, "toughness": 3, "station_threshold": 4, "stationed_keywords": [kw("flying")],
                "triggered_abilities": [{"$t": "TriggeredAbility", "event": "beginning_of_combat",
                    "predicate": {"$t": "Controlled"}, "effect": {"$t": "GainLife", "amount": 2}, "tier_threshold": 2}]}),
        ),
        // A land with station (CR 722.1: no power/toughness box), as the
        // planets of the set are (docs/ir_defects.md 20): "{T}: Add {G}. /
        // Station. / 3+ | {T}: You gain 3 life. / 5+ | {T}: Add {C}{C}."
        card(
            "Lane A Planet",
            "",
            &["Land"],
            &["Planet"],
            json!({"produces": ["G"], "station_threshold": 3, "activated_abilities": [
                {"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true},
                    "effect": {"$t": "GainLife", "amount": 3}, "tier_threshold": 3},
                {"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true},
                    "effect": {"$t": "AddManaToPool", "produces": [["C", 2]]}, "is_mana_ability": true, "tier_threshold": 5}]}),
        ),
    ];
    cards.into_iter().map(|v| serde_json::from_value(v).expect("custom card IR")).collect()
}

/// The Edge of Eternities card table plus lane A's custom test cards.
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

pub fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 20])
        .library(1, &["Forest"; 20])
}

pub fn named(kind: &str) -> Counter {
    Counter::Named(db().subtype_id(&format!("counter:{kind}")).unwrap())
}

pub fn charge() -> Counter {
    named("charge")
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

/// The index of `c`'s station ability (its last activated ability: the
/// lowering appends it after the printed ones).
pub fn station_slot(g: &Game, c: CardIdx) -> u8 {
    (g.def_of(c).activated.len() - 1) as u8
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

/// Station `ship` (P`p`'s permanent) by tapping `crew` and resolve it.
pub fn station(g: &mut Game, ship: CardIdx, crew: CardIdx) {
    let s = station_slot(g, ship);
    assert!(g.has_option(act(ship, s)), "station offered\n{}", log(g));
    g.choose(act(ship, s));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    if g.kind() == Some(DecisionKind::ChoosePermanent) {
        g.choose(Choice::Card(crew));
    }
    settle(g);
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

/// Pass until P`p`'s next main phase 1 with priority and an empty stack.
pub fn next_main(g: &mut Game, p: PlayerId) {
    let now = g.turn().number;
    quiet_until(g, |g| g.turn().number > now && g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty());
}
