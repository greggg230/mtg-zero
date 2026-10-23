//! M7b part 2 (ENGINE.md 25): cards with more than one face, part 1:
//! transforming double-faced cards, modal double-faced cards, Adventures.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn scen() -> Scenario {
    manual().library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Answer everything with first options until a player has priority.
fn pay(g: &mut Game) {
    while !at_priority(g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// Pass priority and take first options until the stack is empty.
fn resolve_all(g: &mut Game) {
    resolve_with(g, |_| None)
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
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
    panic!("resolve_with: no progress");
}

const ODDITY: &str = "Ulvenwald Oddity // Ulvenwald Behemoth";

#[test]
fn ulvenwald_oddity_transforms_into_the_behemoth_cr_701_27() {
    let mut g = scen()
        .battlefield(0, &[ODDITY, "Test Bear"])
        .battlefield(0, &["Forest"; 7])
        .start();
    let o = bf(&g, 0, ODDITY);
    assert_eq!(g.pt(o), (4, 4));
    let r = g.obj_ref(o);
    g.choose(act(o, 0));
    pay(&mut g);
    resolve_all(&mut g);
    // The same object (CR 712.18), now Ulvenwald Behemoth: 8/8, trample,
    // haste; other creatures get +1/+1, trample and haste.
    assert_eq!(g.obj_ref(o), r);
    assert_eq!(g.name(o), "Ulvenwald Behemoth");
    assert_eq!(g.pt(o), (8, 8));
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(bear), (3, 3));
    assert!(g.has_kw(bear, Kw::Trample) && g.has_kw(bear, Kw::Haste));
    // CR 712.8e: its mana value is the front face's; its colour (green)
    // comes from its colour indicator.
    assert_eq!(g.def_of(o).mana_value(), 4);
    assert_eq!(g.colors_of(o), 1 << 4);
    // It has no transform ability any more.
    assert!(g.pending_decision().unwrap().options.iter().all(|x| x.choice != act(o, 0)));
}
