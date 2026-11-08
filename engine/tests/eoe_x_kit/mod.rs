//! The cross-lane pass of the Edge of Eternities port (ENGINE.md 99.8): the
//! test card table (the set's cards plus custom IR test cards, never in the
//! fuzz or digest pools, which `tests/common` feeds) and driving helpers.

#![allow(dead_code)]

use engine::db::{AbilitySlot, CardDb, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
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
    if let Value::Object(m) = extra {
        for (k, v) in m {
            pr[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": pr})
}

pub fn creature(name: &str, mana: &str, p: i64, t: i64, subtypes: &[&str], extra: Value) -> Value {
    let mut v = card(name, mana, &["Creature"], subtypes, extra);
    v["printing"]["power"] = json!(p);
    v["printing"]["toughness"] = json!(t);
    v
}

/// The custom test cards of the cross-lane pass.
pub fn custom_cards() -> Vec<card_ir::Card> {
    let none = json!({});
    let cards = vec![
        creature("Cross Bear", "{1}{G}", 2, 2, &["Bear"], none.clone()),
        creature("Cross Giant", "{4}{G}", 5, 5, &["Giant"], none.clone()),
        creature("Cross Sliver", "{1}{G}", 1, 1, &["Sliver"], none.clone()),
        card("Cross Trinket", "{1}", &["Artifact"], &[], none.clone()),
        card("Cross Totem", "{1}", &["Enchantment"], &[], none.clone()),
        {
            let mut v = creature("Cross Robot", "{2}", 2, 2, &["Robot"], none.clone());
            v["printing"]["card_types"] = json!(["Artifact", "Creature"]);
            v
        },
        // A Spacecraft that is an artifact creature without station, and a
        // noncreature Spacecraft (CR 205.3g: an artifact subtype).
        {
            let mut v = creature("Cross Starship", "{3}", 3, 3, &["Spacecraft"], none.clone());
            v["printing"]["card_types"] = json!(["Artifact", "Creature"]);
            v
        },
        card("Cross Hulk", "{3}", &["Artifact"], &["Spacecraft"], none.clone()),
        // "Cross Bolt deals 10 damage to target creature."
        card("Cross Bolt", "{R}", &["Instant"], &[], json!({"spell_effect": {"$t": "DealDamageToTargetCreature", "amount": 10,
            "targets": [{"$t": "TargetSpec", "filter": {"$t": "IsCreature"}}]}})),
        // "Target creature gets +3/+3 until end of turn."
        card("Cross Growth", "{G}", &["Instant"], &[], json!({"spell_effect": {"$t": "PumpTargetCreatureUntilEOT",
            "power_bonus": 3, "toughness_bonus": 3}})),
    ];
    cards.into_iter().map(|v| serde_json::from_value(v).expect("custom card IR")).collect()
}

/// The Edge of Eternities card table plus the custom test cards.
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

pub fn charge() -> Counter {
    Counter::Named(db().subtype_id("counter:charge").unwrap())
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

pub fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

pub fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// The index of `c`'s station ability (its last activated ability: the
/// lowering appends it after the printed ones; ENGINE.md 99.2).
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

/// To the active player's end step this turn, with its triggers waiting or
/// on the stack.
pub fn to_end_step(g: &mut Game) {
    let t = g.turn().number;
    quiet_until(g, |g| g.turn().number == t && g.turn().step == Step::End && (at_priority(g) || g.kind() == Some(DecisionKind::OrderTriggers) || g.kind() == Some(DecisionKind::Target)));
}

/// Station `ship` by tapping `crew` and resolve it (CR 702.184a).
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

/// Pass priority until P`p` declares attackers, then attack the opponent
/// with `names`.
pub fn attack_with(g: &mut Game, p: PlayerId, names: &[&str]) {
    for _ in 0..100 {
        g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || at_priority(g));
        if g.kind() == Some(DecisionKind::DeclareAttackers) {
            break;
        }
        g.pass();
    }
    for n in names {
        let c = bf(g, p, n);
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

/// Give P1 priority in P0's turn with an empty stack (P0 passes once).
pub fn to_opponent_priority(g: &mut Game) {
    g.pass();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
}
