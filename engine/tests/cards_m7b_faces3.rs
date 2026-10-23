//! M7b part 2 (ENGINE.md 25): cards with more than one face, part 3: Sagas
//! (CR 714): Urza's Saga, and Fable of the Mirror-Breaker's chapters (its
//! Reflection of Kiki-Jiki's copy ability: `cards_m7b_copies.rs`, M7b part
//! 3).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn pay(g: &mut Game) {
    g.drive_until(|g| {
        at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
    });
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| {
            at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
        });
    }
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
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
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
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
    panic!("quiet_until: no progress");
}

fn lore(g: &Game, c: CardIdx) -> u16 {
    g.counters_on(c, Counter::Lore)
}

/// P0's precombat main phase of turn `n`, with the turn-based lore counter
/// put and the chapter ability waiting or on the stack.
fn main_of(g: &mut Game, n: u32) {
    quiet_until(g, |g| g.turn().number == n && g.turn().step == Step::Main1 && at_priority(g));
}

// ---- Urza's Saga -----------------------------------------------------------------

const SAGA: &str = "Urza's Saga";

#[test]
fn urzas_saga_chapters_cr_714() {
    // "(As this Saga enters and after your draw step, add a lore counter.
    // Sacrifice after III.) I - This Saga gains '{T}: Add {C}.' II - This
    // Saga gains '{2}, {T}: Create a 0/0 colorless Construct artifact
    // creature token with "This token gets +1/+1 for each artifact you
    // control."' III - Search your library for an artifact card with mana
    // cost {0} or {1}, put it onto the battlefield, then shuffle."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Island", "Island", "Mind Stone", "Mox Pearl", "Sol Ring", "Island"])
        .library(1, &["Forest"; 10])
        .hand(0, &[SAGA, "Sol Ring"])
        .start();
    let s = g.card(0, SAGA, Zone::Hand);
    // A land (an Enchantment Land - Urza's Saga): played as the land drop.
    g.choose(Choice::Play(s));
    // CR 714.3a: it enters with a lore counter; chapter I triggers (714.2b).
    assert_eq!(lore(&g, s), 1);
    g.drive_until(|g| !g.waiting_triggers().is_empty() || !g.stack().is_empty());
    // No mana ability yet (not gained until chapter I resolves).
    assert_eq!(g.def_of(s).latent, 2);
    settle(&mut g);
    // Gained "{T}: Add {C}": it pays for Sol Ring.
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    assert!(g.has_option(Choice::Cast(ring)));
    g.choose(Choice::Cast(ring));
    pay(&mut g);
    settle(&mut g);
    assert!(g.obj(s).tapped);
    assert_eq!(g.obj(bf(&g, 0, "Sol Ring")).zone, Zone::Battlefield);
    // Turn 5: a lore counter as the precombat main phase begins (714.3c):
    // chapter II.
    main_of(&mut g, 5);
    assert_eq!(lore(&g, s), 2);
    settle(&mut g);
    // "{2}, {T}: Create a Construct": activated ability 1 (the {C} is 0).
    let act = Choice::Ability(s, AbilitySlot::Activated(1));
    assert!(g.has_option(act));
    g.choose(act);
    pay(&mut g);
    settle(&mut g);
    let construct = bf(&g, 0, "Construct");
    // Two artifacts (Sol Ring, itself).
    assert_eq!(g.pt(construct), (2, 2));
    // Turn 7: chapter III; while it is on the stack the Saga stays (CR
    // 714.4: not while its chapter ability is waiting or on the stack).
    main_of(&mut g, 7);
    assert_eq!(lore(&g, s), 3);
    g.drive_until(|g| !g.stack().is_empty() || !at_priority(g));
    assert_eq!(g.obj(s).zone, Zone::Battlefield);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => {
            let names: Vec<String> = g
                .pending_decision()
                .unwrap()
                .options
                .iter()
                .filter_map(|o| match o.choice {
                    Choice::Def(d) => Some(db().def(d).name.clone()),
                    _ => None,
                })
                .collect();
            // Mana cost {0} or {1}: Mox Pearl, Sol Ring; not Mind Stone ({2})
            // nor a land (no mana cost).
            assert_eq!(names, vec!["Mox Pearl".to_string(), "Sol Ring".to_string()]);
            g.pending_decision().unwrap().options.iter().find(|o| matches!(o.choice, Choice::Def(d) if db().def(d).name == "Mox Pearl")).map(|o| o.choice)
        }
        _ => None,
    });
    assert_eq!(g.count_in(0, "Mox Pearl", Zone::Battlefield), 1);
    // CR 704.5s: sacrificed after the last chapter resolved.
    assert_eq!(g.obj(s).zone, Zone::Graveyard);
}

#[test]
fn a_saga_isnt_sacrificed_while_its_last_chapter_waits_cr_704_5s() {
    // Urza's Saga with three lore counters and its chapter III trigger still
    // to be put on the stack, then on the stack: it stays until it resolved.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[SAGA])
        .counters(0, SAGA, Counter::Lore, 1)
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, SAGA);
    // (The scenario's counter triggered chapter I, the precombat main
    // phase's lore counter chapter II: resolve them.)
    assert_eq!(lore(&g, s), 2);
    settle(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Battlefield);
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && !g.stack().is_empty());
    assert_eq!(lore(&g, s), 3);
    assert_eq!(g.obj(s).zone, Zone::Battlefield);
    settle(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Graveyard);
}

// ---- Fable of the Mirror-Breaker -------------------------------------------------------

const FABLE: &str = "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki";

#[test]
fn fable_is_supported_since_token_copies_m7b_part_3() {
    assert!(engine::db::CardDb::cube().expect_id(FABLE).is_ok());
}

#[test]
fn fable_chapters_and_reflection_cr_714_712_14a() {
    // "I - Create a 2/2 red Goblin Shaman creature token with 'Whenever this
    // token attacks, create a Treasure token.' II - You may discard up to two
    // cards. If you do, draw that many cards. III - Exile this Saga, then
    // return it to the battlefield transformed under your control."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Mountain"; 3])
        .hand(0, &[FABLE, "Forest", "Island"])
        .start();
    let f = g.card(0, FABLE, Zone::Hand);
    g.choose(Choice::Cast(f));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(lore(&g, f), 1);
    let shaman = bf(&g, 0, "Goblin Shaman");
    assert_eq!(g.pt(shaman), (2, 2));
    assert_eq!(g.colors_of(shaman), 1 << 3);
    // Turn 5: chapter II: discard one (a Forest), draw one.
    main_of(&mut g, 5);
    let forest = g.card(0, "Forest", Zone::Hand);
    let hand = g.player(0).hand.len();
    let picked = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Discard) => {
            picked.set(picked.get() + 1);
            assert!(g.has_option(Choice::Pass), "up to two");
            Some(if picked.get() == 1 { Choice::Card(forest) } else { Choice::Pass })
        }
        _ => None,
    });
    assert_eq!(g.obj(forest).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), hand, "drew that many: one");
    // The Goblin Shaman's attack makes a Treasure.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(shaman));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Treasure", Zone::Battlefield), 1);
    // Turn 7: chapter III: exiled and returned transformed (a new object);
    // not sacrificed (Reflection of Kiki-Jiki is no Saga).
    main_of(&mut g, 7);
    settle(&mut g);
    let r = bf(&g, 0, "Reflection of Kiki-Jiki");
    assert_eq!(r, f);
    assert!(g.is_creature(r) && g.is_type(r, engine::db::types::ENCHANTMENT));
    assert_eq!(g.pt(r), (2, 2));
    assert_eq!(g.colors_of(r), 1 << 3);
    assert_eq!(lore(&g, r), 0, "a new object: no lore counters");
    quiet_until(&mut g, |g| g.turn().number == 8);
    assert_eq!(g.obj(r).zone, Zone::Battlefield);
}
