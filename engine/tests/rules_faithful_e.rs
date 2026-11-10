//! Loose ends of the faithfulness audit (docs/faithfulness_audit.md 4, 7;
//! ENGINE.md 100.6): day and night (CR 702.145, 732) and the load-time bound
//! on companions (audit N13). Each test names the CR rule it checks; those
//! marked "failed before" failed before the fix.

mod common;

use engine::db::{AbilitySlot, CardDb};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

const LIBERATOR: &str = "Outland Liberator // Frenzied Trapbreaker";
const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";
const OCULUS: &str = "Abhorrent Oculus";
const WOODLAND: &str = "Shifting Woodland";

/// The cube, the Edge of Eternities cards it doesn't have and the shared
/// test cards, lowered once (Xu-Ifit and Robe of Stars are Edge of
/// Eternities cards; Outland Liberator is the cube's daybound card).
fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        for c in engine::db::load_eoe_document().cards {
            if !cards.iter().any(|x| x.name == c.name) {
                cards.push(c);
            }
        }
        cards.extend(common::custom_cards());
        CardDb::from_cards(&cards).leak()
    })
}

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Run the stack down; `answer` answers what is asked (else the first
/// option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g) || g.is_over());
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
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

/// Answer a target decision with `c` (else the first option).
fn target(c: CardIdx) -> impl Fn(&Game) -> Option<Choice> {
    move |g: &Game| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(c))).then(|| g.tgt(c))
}

/// Pass priority (and decline attacks and blocks; the first option
/// otherwise) until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..20_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("game over\n{}", log(g)));
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress\n{}", log(g));
}

/// Player `p` has priority in turn `n`.
fn priority_of(g: &Game, p: PlayerId, n: u32) -> bool {
    g.turn().number == n && at_priority(g) && g.pending_decision().unwrap().player == p
}

fn front(g: &Game, c: CardIdx) -> bool {
    g.name(c).starts_with("Outland Liberator")
}

// ---- 702.145b: a daybound card enters transformed at night ------------------------------

/// "Daybound" means "If it is night and this permanent is represented by a
/// double-faced card, it enters transformed" (CR 702.145b): a daybound spell
/// cast at night enters with its nightbound face up; it doesn't enter with
/// its daybound face up and then transform (ruling 483aa586). The engine
/// already did this (the face is set before the move); this pins it with the
/// transformation count and the log.
#[test]
fn cr_702_145b_a_daybound_spell_cast_at_night_enters_transformed_without_transforming() {
    // ruling: 483aa586
    let mut g = scen().battlefield(0, &["Forest", "Forest"]).hand(0, &[LIBERATOR]).day(Some(false)).start();
    let l = g.card(0, LIBERATOR, Zone::Hand);
    g.choose(Choice::Cast(l));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(l).zone, Zone::Battlefield);
    assert_eq!(g.name(l), "Frenzied Trapbreaker");
    assert_eq!(g.obj(l).flips, 0, "it entered with that face up\n{}", log(&g));
    assert!(log(&g).contains("Frenzied Trapbreaker Stack -> Battlefield"), "{}", log(&g));
    assert!(!log(&g).contains("transforms"), "{}", log(&g));
}

/// The same for a daybound card put onto the battlefield at night (ruling
/// 9d7e7537). Pinned as above.
#[test]
fn cr_702_145b_a_daybound_card_put_onto_the_battlefield_at_night_enters_transformed() {
    // ruling: 9d7e7537
    let mut g = scen().battlefield(0, &["Swamp"]).graveyard(0, &[LIBERATOR]).hand(0, &["Reanimate"]).day(Some(false)).start();
    let l = g.card(0, LIBERATOR, Zone::Graveyard);
    let r = g.card(0, "Reanimate", Zone::Hand);
    g.choose(Choice::Cast(r));
    resolve_with(&mut g, target(l));
    assert_eq!(g.obj(l).zone, Zone::Battlefield);
    assert_eq!(g.name(l), "Frenzied Trapbreaker");
    assert_eq!(g.obj(l).flips, 0, "{}", log(&g));
    assert!(log(&g).contains("Frenzied Trapbreaker Graveyard -> Battlefield"), "{}", log(&g));
    assert!(!log(&g).contains("transforms"), "{}", log(&g));
}

/// 702.145b's "enters transformed" is an ability of the entering permanent,
/// applied by its characteristics as it would exist on the battlefield (CR
/// 614.12): returned by Xu-Ifit ("It's a Skeleton ... and has no
/// abilities"), Outland Liberator has no daybound as it enters, so it enters
/// front face up, and with no daybound it stays so at night (702.145c needs
/// daybound). Failed before: it entered as Frenzied Trapbreaker.
#[test]
fn cr_702_145b_a_daybound_card_entering_with_no_abilities_at_night_enters_front_face_up() {
    let mut g = scen().battlefield(0, &["Xu-Ifit, Osteoharmonist"]).graveyard(0, &[LIBERATOR]).day(Some(false)).start();
    let xu = bf(&g, 0, "Xu-Ifit, Osteoharmonist");
    let l = g.card(0, LIBERATOR, Zone::Graveyard);
    g.choose(act(xu, 0));
    resolve_with(&mut g, target(l));
    assert_eq!(g.obj(l).zone, Zone::Battlefield, "{}", log(&g));
    assert!(front(&g, l), "front face up: {}\n{}", g.name(l), log(&g));
    assert_eq!(g.pt(l), (2, 2));
    assert_eq!(g.obj(l).flips, 0);
    assert_eq!(g.day(), Some(false));
}

/// A token is not "represented by a double-faced card" (CR 702.145b): a
/// double-faced token that is a copy of a daybound permanent front face up
/// (CR 707.8a; Outland Liberator turned into an Elk by Oko, so it kept its
/// front face when night came) enters front face up at night and transforms
/// at once (702.145c), after it is seen entering. Failed before: the token
/// entered as Frenzied Trapbreaker without transforming.
#[test]
fn cr_702_145c_a_double_faced_token_with_daybound_enters_front_face_up_at_night_then_transforms() {
    let mut g = scen()
        .battlefield(0, &["Oko, Thief of Crowns", LIBERATOR, FABLE, "Mountain", "Mountain"])
        .transformed(0, FABLE)
        .day(Some(true))
        .start();
    let oko = bf(&g, 0, "Oko, Thief of Crowns");
    let l = bf(&g, 0, LIBERATOR);
    // Oko's +1: "Target artifact or creature loses all abilities and becomes
    // a green Elk creature with base power and toughness 3/3."
    g.choose(act(oko, 1));
    resolve_with(&mut g, target(l));
    assert_eq!(g.pt(l), (3, 3), "an Elk\n{}", log(&g));
    // P0 casts no spell this turn: night as turn 4 begins (CR 732.2a); the
    // Elk has no daybound and keeps its front face.
    quiet_until(&mut g, |g| priority_of(g, 0, 4));
    assert_eq!(g.day(), Some(false));
    assert!(front(&g, l) && g.obj(l).flips == 0, "{}", log(&g));
    // Reflection of Kiki-Jiki copies it: the copiable values are Outland
    // Liberator's front face (Oko's effect isn't copied, 707.2).
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    g.choose(act(r, 0));
    resolve_with(&mut g, target(l));
    let tok = g.permanents_of(0).find(|&c| c != l && g.is_token(c)).unwrap_or_else(|| panic!("a token\n{}", log(&g)));
    assert_eq!(g.name(tok), "Frenzied Trapbreaker", "{}", log(&g));
    assert_eq!(g.obj(tok).flips, 1, "it entered front face up and transformed\n{}", log(&g));
    let lines = g.log_lines();
    let entered = lines.iter().position(|s| s.contains("Nowhere -> Battlefield")).expect("the token enters");
    assert!(lines[entered].contains("Outland Liberator"), "seen entering front face up: {}", lines[entered]);
    let turned = lines.iter().position(|s| s.contains("Frenzied Trapbreaker transforms")).expect("then transforms");
    assert!(entered < turned);
    assert!(front(&g, l), "the Elk is untouched");
}

// ---- 702.145c-d, f: "any time" ----------------------------------------------------------

/// P0's Oculus manifests dread at P1's upkeep from a library whose top two
/// cards are Outland Liberator (manifested) and an Island; returns at P0's
/// first priority after, with the face-down card.
fn manifested_liberator(day: Option<bool>) -> (Game, CardIdx) {
    let mut g = scen()
        .library(0, &["Island", LIBERATOR])
        .battlefield(0, &[OCULUS, "Forest", "Forest"])
        .day(day)
        .at(Step::End, 0)
        .start();
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::PutOntoBattlefield));
    let c = g.card(0, LIBERATOR, Zone::Library);
    g.choose(Choice::Card(c));
    quiet_until(&mut g, |g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(g.is_face_down(c));
    (g, c)
}

/// Turn the face-down `c` face up for its mana cost (CR 701.40b, a special
/// action).
fn turn_face_up(g: &mut Game, c: CardIdx) {
    assert!(g.has_option(act(c, 0)), "{:?}", g.options_desc());
    g.choose(act(c, 0));
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while g.kind() == Some(DecisionKind::PayMana) {
        let o = g.pending_decision().unwrap().options[0].choice;
        g.choose(o);
        g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    }
    assert!(!g.is_face_down(c), "{}", log(g));
}

/// A manifested daybound card turned face up has its front face up (CR
/// 712.12a). At night, "any time a player controls a permanent that is front
/// face up with daybound and it's night, that player transforms that
/// permanent. This happens immediately" (702.145c). Failed before: it stayed
/// Outland Liberator at night.
#[test]
fn cr_702_145c_a_daybound_card_turned_face_up_at_night_transforms_at_once() {
    let (mut g, c) = manifested_liberator(Some(false));
    turn_face_up(&mut g, c);
    assert_eq!(g.name(c), "Frenzied Trapbreaker", "{}", log(&g));
    assert_eq!(g.obj(c).flips, 1);
}

/// "Any time a player controls a permanent with daybound, if it's neither day
/// nor night, it becomes day" (CR 702.145d): turned face up, not entering.
/// Failed before: it stayed neither.
#[test]
fn cr_702_145d_a_daybound_card_turned_face_up_makes_it_day() {
    let (mut g, c) = manifested_liberator(None);
    assert_eq!(g.day(), None);
    turn_face_up(&mut g, c);
    assert!(front(&g, c));
    assert_eq!(g.day(), Some(true), "{}", log(&g));
}

/// Shifting Woodland becoming a copy of Outland Liberator (a card in a
/// graveyard has its front face's characteristics, CR 712.6a) has daybound:
/// with neither day nor night, it becomes day (702.145d). Failed before: it
/// stayed neither.
#[test]
fn cr_702_145d_a_permanent_that_becomes_a_copy_of_a_daybound_card_makes_it_day() {
    let mut g = scen()
        .battlefield(0, &[WOODLAND, "Forest", "Forest", "Forest", "Forest"])
        .graveyard(0, &[LIBERATOR, "Lightning Bolt", "Mox Pearl", "Island"])
        .start();
    let w = bf(&g, 0, WOODLAND);
    let l = g.card(0, LIBERATOR, Zone::Graveyard);
    let slot = (0..g.def_of(w).activated.len() as u8).find(|&i| g.has_option(act(w, i))).expect("delirium");
    g.choose(act(w, slot));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(l)))),
        Some(DecisionKind::PayMana) => {
            g.pending_decision().unwrap().options.iter().find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == w)).map(|o| o.choice)
        }
        _ => None,
    });
    assert!(front(&g, w), "a copy of the card's front face: {}\n{}", g.name(w), log(&g));
    assert_eq!(g.day(), Some(true), "{}", log(&g));
}

/// E2. Shifting Woodland as Outland Liberator is a single-faced card with the
/// Liberator's front face as its copiable values. Reflection of Kiki-Jiki's
/// token copy of it at night has daybound, but it is a copy of a permanent
/// that isn't a transforming permanent: the token is single-faced (CR 707.8a
/// covers only copies of transforming permanents) and isn't represented by a
/// double-faced card, so it can't transform (701.28c) and stays Outland
/// Liberator. A token copy of the Liberator card itself is double-faced
/// (`cr_702_145c_a_double_faced_token_...` above). Failed before: the token
/// transformed into Frenzied Trapbreaker.
#[test]
fn cr_707_8a_a_token_copy_of_a_single_faced_permanent_copying_a_daybound_card_cant_transform() {
    let mut g = scen()
        .battlefield(0, &[WOODLAND, FABLE, "Forest", "Forest", "Forest", "Forest", "Forest"])
        .transformed(0, FABLE)
        .graveyard(0, &[LIBERATOR, "Lightning Bolt", "Mox Pearl", "Island"])
        .day(Some(false))
        .start();
    let w = bf(&g, 0, WOODLAND);
    let l = g.card(0, LIBERATOR, Zone::Graveyard);
    let pay = move |g: &Game, t: CardIdx| match g.kind() {
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Obj(g.obj_ref(t)))),
        Some(DecisionKind::PayMana) => {
            g.pending_decision().unwrap().options.iter().find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == w)).map(|o| o.choice)
        }
        _ => None,
    };
    let slot = (0..g.def_of(w).activated.len() as u8).find(|&i| g.has_option(act(w, i))).expect("delirium");
    g.choose(act(w, slot));
    resolve_with(&mut g, |g| pay(g, l));
    assert!(front(&g, w) && g.obj(w).flips == 0, "Woodland as the Liberator's front face: {}\n{}", g.name(w), log(&g));
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    g.choose(act(r, 0));
    resolve_with(&mut g, |g| pay(g, w));
    let tok = g.permanents_of(0).find(|&c| g.is_token(c)).unwrap_or_else(|| panic!("a token\n{}", log(&g)));
    assert_eq!(g.day(), Some(false));
    assert!(front(&g, tok), "the token keeps the face it was created as: {}\n{}", g.name(tok), log(&g));
    assert_eq!(g.obj(tok).flips, 0, "{}", log(&g));
    assert!(!log(&g).contains("transforms"), "{}", log(&g));
}

/// At night, Frenzied Trapbreaker loses all its abilities while The Wondrous
/// Wasp remains; it becomes day, and with no nightbound it keeps its back
/// face. When the Wasp leaves, it has nightbound again, back face up by day:
/// it transforms at once (CR 702.145f). Failed before: it stayed Frenzied
/// Trapbreaker by day.
#[test]
fn cr_702_145f_a_nightbound_permanent_that_regains_nightbound_by_day_transforms_at_once() {
    let mut g = scen()
        .battlefield(0, &[LIBERATOR, "Mountain"])
        .transformed(0, LIBERATOR)
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island", "Mountain"])
        .hand(1, &["The Wondrous Wasp", "Lightning Bolt"])
        .day(Some(false))
        .at(Step::Main1, 1)
        .start();
    let l = bf(&g, 0, "Frenzied Trapbreaker");
    // P1's turn: the Wasp (its enters ability taps and silences the
    // Trapbreaker), then a second spell, so it becomes day as P0's turn
    // begins (CR 732.2b).
    let wasp = g.card(1, "The Wondrous Wasp", Zone::Hand);
    g.choose(Choice::Cast(wasp));
    resolve_with(&mut g, target(l));
    assert_eq!(g.obj(wasp).zone, Zone::Battlefield, "{}", log(&g));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(0))));
    let n = g.turn().number;
    quiet_until(&mut g, |g| priority_of(g, 0, n + 1) && g.turn().step == Step::Main1);
    assert_eq!(g.day(), Some(true), "{}", log(&g));
    assert_eq!(g.name(l), "Frenzied Trapbreaker", "no nightbound while the Wasp remains");
    // P0 kills the Wasp.
    let b0 = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b0));
    resolve_with(&mut g, target(wasp));
    assert_eq!(g.obj(wasp).zone, Zone::Graveyard, "{}", log(&g));
    assert!(front(&g, l), "it transformed once it had nightbound again: {}\n{}", g.name(l), log(&g));
    assert_eq!(g.obj(l).flips, 1);
}

/// A daybound permanent phased out by day doesn't exist as it becomes night
/// (CR 702.26b); it phases in front face up at night and transforms at once
/// (702.145c). Failed before: it stayed Outland Liberator at night.
#[test]
fn cr_702_145c_a_daybound_permanent_phasing_in_at_night_transforms_at_once() {
    let mut g = scen()
        .battlefield(0, &["Robe of Stars", LIBERATOR, "Plains", "Plains", "Plains"])
        .day(Some(true))
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let robe = bf(&g, 0, "Robe of Stars");
    let l = bf(&g, 0, LIBERATOR);
    // Equip {1}, then "{1}{W}: Equipped creature phases out."
    g.choose(act(robe, 1));
    resolve_with(&mut g, target(l));
    assert_eq!(g.obj(robe).attached.map(|h| h.idx), Some(l), "{}", log(&g));
    g.choose(act(robe, 0));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(l).zone, Zone::PhasedOut, "{}", log(&g));
    // P0 cast no spell: night as turn 4 begins.
    quiet_until(&mut g, |g| priority_of(g, 1, 4));
    assert_eq!(g.day(), Some(false));
    assert!(front(&g, l), "phased out: not transformed");
    // P0's untap step: it phases in (CR 502.1) at night.
    quiet_until(&mut g, |g| priority_of(g, 0, 5));
    assert_eq!(g.obj(l).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.day(), Some(false));
    assert_eq!(g.name(l), "Frenzied Trapbreaker", "{}", log(&g));
    assert_eq!(g.obj(l).flips, 1);
}

// ---- N13: at most 64 companions a table, checked as it loads -----------------------------

/// A creature card with Lutri's companion condition ("each nonland card in
/// your starting deck has a different name").
fn companion_card(k: usize) -> card_ir::Card {
    let name = format!("E Companion {k}");
    let printing: Value = json!({
        "$t": "CardPrinting", "name": name, "card_types": ["Creature"], "subtypes": [], "supertypes": [],
        "power": 1, "toughness": 1, "mana_cost": {"$t": "ManaCost", "generic": 1}, "mana_cost_str": "{1}",
        "type_line": "Creature",
        "companion": {"$t": "Companion", "condition": {"$t": "StartingDeckDifferentNames",
            "cards": {"$t": "ObjectNot", "inner": {"$t": "IsLand"}}}},
    });
    serde_json::from_value(json!({"name": name, "custom": true, "printing": printing})).expect("companion card IR")
}

/// The revealed-companion decision offers each candidate on a `LIBRARY`
/// slot (64), so a table may have at most 64 companion definitions: 64 load.
#[test]
fn n13_a_table_with_64_companions_loads() {
    let cards: Vec<card_ir::Card> = (0..64).map(companion_card).collect();
    let db = CardDb::from_cards(&cards);
    for k in 0..64 {
        let d = db.expect_id(&format!("E Companion {k}")).unwrap_or_else(|e| panic!("{e}"));
        assert!(db.def(d).companion.is_some());
    }
}

/// ... and a 65th fails loudly as the table loads (it was cut silently from
/// the decision: `take(64)`).
#[test]
#[should_panic(expected = "companion")]
fn n13_a_table_with_65_companions_fails_loudly_at_load() {
    let cards: Vec<card_ir::Card> = (0..65).map(companion_card).collect();
    let _ = CardDb::from_cards(&cards);
}
