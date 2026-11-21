//! Lane C of the Legacy Hogaak Tempo cards (ENGINE.md 220.3) against their
//! Oracle text, the Comprehensive Rules and their official rulings (`//
//! ruling: <hash8>`, `data/rulings/legacy.rulings.json`). They are in the card
//! table of constructed play only (`CardDb::constructed`).
//!
//! Stitcher's Supplier {B}, Creature — Zombie 1/1: "When this creature enters
//! or dies, mill three cards. (Put the top three cards of your library into
//! your graveyard.)"
//!
//! Dark Matter Manipulator {B}, Creature — Human Warlock 1/2: "When this
//! creature enters, mill three cards. / This creature gets +2/+0 for every
//! seven cards in your graveyard."
//!
//! Moonshadow {B}, Creature — Elemental 7/7: "Menace / This creature enters
//! with six -1/-1 counters on it. / Whenever one or more permanent cards are
//! put into your graveyard from anywhere while this creature has a -1/-1
//! counter on it, remove a -1/-1 counter from this creature."
//!
//! Dryad Arbor, Land Creature — Forest Dryad 1/1 (a green colour indicator):
//! "(This land isn't a spell, it's affected by summoning sickness, and it has
//! "{T}: Add {G}.")"

use engine::db::{types, AbilitySlot, CardDb, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const SUPPLIER: &str = "Stitcher's Supplier";
const DMM: &str = "Dark Matter Manipulator";
const MOON: &str = "Moonshadow";
const ARBOR: &str = "Dryad Arbor";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

/// No auto-answers; empty libraries (each test gives its own).
fn bare() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn hand(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Hand)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Answer the decisions of a cast or an activation until priority: `answer`
/// first, else the first option.
fn pay_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority\n{}", log(g));
}

fn pay(g: &mut Game) {
    pay_with(g, |_| None);
}

/// Resolve the stack (passing priority), `answer` answering what is asked
/// (else the first option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
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

fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = hand(g, p, name);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    pay(g);
}

fn cast_and_resolve(g: &mut Game, p: PlayerId, name: &str) {
    cast(g, p, name);
    resolve(g);
}

fn gy(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).graveyard.iter().map(|&c| g.name(c).to_string()).collect()
}

fn m1(g: &Game, c: CardIdx) -> u64 {
    g.counters_on(c, Counter::M1M1) as u64
}

fn pt(g: &Game, c: CardIdx) -> (i128, i128) {
    let (p, t) = g.pt(c);
    (p as i128, t as i128)
}

// ---- the definitions -------------------------------------------------------------

#[test]
fn the_four_cards_are_defined_as_printed() {
    let d = db();
    let s = d.def(d.id(SUPPLIER).unwrap());
    assert_eq!((s.power, s.toughness, s.mana_value()), (Some(1), Some(1), 1));
    assert_eq!(s.triggers.len(), 2, "enters, dies");
    let m = d.def(d.id(DMM).unwrap());
    assert_eq!((m.power, m.toughness, m.mana_value()), (Some(1), Some(2), 1));
    assert_eq!(m.statics.len(), 1);
    let ms = d.def(d.id(MOON).unwrap());
    assert_eq!((ms.power, ms.toughness, ms.mana_value()), (Some(7), Some(7), 1));
    assert!(ms.keywords.has(Kw::Menace));
    assert_eq!(ms.enters_counters, vec![(Counter::M1M1, 6)]);
    let a = d.def(d.id(ARBOR).unwrap());
    assert_eq!(a.types, types::LAND | types::CREATURE);
    assert_eq!((a.power, a.toughness, a.mana_value()), (Some(1), Some(1), 0));
    assert_eq!(a.colors, 1 << engine::mana::G);
}

// ---- smoke ---------------------------------------------------------------------

#[test]
fn smoke_supplier_mills_and_moonshadow_shrinks() {
    let mut g = bare()
        .library(0, &["Island", "Lightning Bolt", "Forest", "Island", "Island"])
        .hand(0, &[SUPPLIER])
        .battlefield(0, &["Swamp", MOON])
        .counters(0, MOON, Counter::M1M1, 6)
        .start();
    cast_and_resolve(&mut g, 0, SUPPLIER);
    assert_eq!(gy(&g, 0), vec!["Island", "Lightning Bolt", "Forest"], "{}", log(&g));
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 5, "{}", log(&g));
}

#[test]
fn smoke_dryad_arbor_is_played_not_cast() {
    let mut g = bare().hand(0, &[ARBOR]).start();
    let a = hand(&g, 0, ARBOR);
    assert!(g.has_option(Choice::Play(a)) && !g.has_option(Choice::Cast(a)), "{}", opts(&g));
    g.choose(Choice::Play(a));
    let a = bf(&g, 0, ARBOR);
    assert!(g.is_creature(a) && g.is_type(a, types::LAND));
    assert!(!g.offered().iter().any(|c| matches!(c, Choice::Ability(x, AbilitySlot::Activated(_)) if *x == a)), "{}", opts(&g));
    let _ = Step::Main1;
}
