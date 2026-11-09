//! Workstream A of the faithfulness audit (ENGINE.md 100.2,
//! docs/faithfulness_audit.md 5.1-5.2): every legal option of a choice is
//! reachable inside the fixed action ranges, by paging (`Game::ask_paged`)
//! or by a big number's digits (`Game::ask_number`), with nothing truncated
//! or clamped. Each test names the CR rule whose choice it checks.

mod common;

use engine::db::CardDb;
use engine::decision::act;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step};
use std::sync::OnceLock;

/// The cube's cards and the Edge of Eternities' (as `rules_faithful_c.rs`).
fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        for c in engine::db::load_eoe_document().cards {
            if !cards.iter().any(|x| x.name == c.name) {
                cards.push(c);
            }
        }
        Box::leak(Box::new(CardDb::from_cards(&cards)))
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

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

fn actions(g: &Game) -> Vec<u16> {
    g.pending_decision().unwrap().options.iter().map(|o| o.action).collect()
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// The pending decision's "next page" option (ENGINE.md 100.1).
fn next_page(g: &Game) -> Choice {
    options(g).into_iter().find(|c| matches!(c, Choice::Page(_))).unwrap_or_else(|| panic!("no next page: {:?}", g.options_desc()))
}

/// A determinized world of the decider asks the same decision (ENGINE.md
/// 10, 100.1: the page or the digits live in the asking task).
fn same_after_determinize(g: &Game) {
    let p = g.pending_decision().unwrap().player;
    let mut d = g.clone();
    d.determinize(p, &mut engine::rng::Rng::new(11));
    assert_eq!(d.pending_decision(), g.pending_decision(), "determinize changed the decision");
}

// ---- 31.7.8: every distinct waiting trigger may be put on the stack first (CR 603.3b) ----

/// Eighteen Ouroboroids ("at the beginning of combat on
/// your turn, put X +1/+1 counters on each creature you control, where X is
/// this creature's power"): eighteen triggers that read their own source,
/// none interchangeable (CR 603.3b, 113.7), so their controller may put any
/// of them on the stack first. The order decision offers the first 15 at
/// `TRIGGER` + 0..=14 and "next page" at `TRIGGER` + 15, then the other
/// three at `TRIGGER` + 0..=2 (ENGINE.md 100.2; before it, the 17th and 18th
/// could never go first, docs/shortcuts.md 31.7.8).
#[test]
fn cr_603_3b_any_of_eighteen_distinct_triggers_goes_on_the_stack_first() {
    let mut g = scen().battlefield(0, &["Ouroboroid"; 18]).at(Step::Main1, 0).start();
    let os: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Ouroboroid").collect();
    assert_eq!(os.len(), 18);
    g.pass();
    g.drive_until(|g| g.kind() == Some(DecisionKind::OrderTriggers));
    assert_eq!(actions(&g), (act::TRIGGER..act::TRIGGER + 16).collect::<Vec<_>>(), "{:?}", g.options_desc());
    assert!(matches!(options(&g)[15], Choice::Page(_)));
    g.choose(next_page(&g));
    same_after_determinize(&g);
    assert_eq!(g.kind(), Some(DecisionKind::OrderTriggers));
    assert_eq!(actions(&g), vec![act::TRIGGER, act::TRIGGER + 1, act::TRIGGER + 2]);
    assert_eq!(options(&g), vec![Choice::Trigger(15), Choice::Trigger(16), Choice::Trigger(17)]);
    let last = g.waiting_triggers().iter().filter(|t| t.controller == 0).nth(17).unwrap().source;
    g.choose(Choice::Trigger(17));
    g.drive_until(|g| g.kind() == Some(DecisionKind::OrderTriggers));
    match g.stack()[0].kind {
        engine::game::StackKind::Ability { source, .. } => assert_eq!(source, last, "the 18th trigger went first\n{}", log(&g)),
        k => panic!("{k:?}"),
    }
    // The next order decision starts at the first page again: 17 distinct
    // triggers, the first page's 15 and "next page".
    assert_eq!(actions(&g), (act::TRIGGER..act::TRIGGER + 16).collect::<Vec<_>>());
}

// ---- 31.7.5, N14: any card name (CR 201.3) ------------------------------------------------

/// Anointed Peacekeeper ("as this enters, look at an opponent's hand, then
/// choose any card name ... activated abilities of sources with the chosen
/// name cost {2} more"): the names are those of every card in the game, the
/// chooser's own too (the tax is on every source's abilities), 72 here: the
/// first page offers 63 at `LIBRARY` + 0..=62, "next page" at `LIBRARY` + 63
/// and `PASS` (a name of no card in the game); the second the other 9 at
/// `LIBRARY` + 0..=8 (ENGINE.md 100.2; before it the opponent's names only,
/// cut at 64: docs/shortcuts.md 31.7.5, the audit's N14). One of the
/// chooser's own cards' names is chosen.
#[test]
fn cr_201_3_peacekeeper_may_name_any_card_in_the_game_even_its_controllers() {
    let names: Vec<&str> = db()
        .supported()
        .filter(|n| !["Anointed Peacekeeper", "Plains", "Island"].contains(n) && db().id(n).is_some_and(|d| !db().def(d).is(engine::db::types::LAND)))
        .take(70)
        .collect();
    assert_eq!(names.len(), 70);
    let mut g = Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &names[30..])
        .library(1, &names[..30])
        .hand(0, &["Anointed Peacekeeper"])
        .battlefield(0, &["Plains"; 3])
        .at(Step::Main1, 0)
        .start();
    let pk = g.card(0, "Anointed Peacekeeper", engine::Zone::Hand);
    g.choose(Choice::Cast(pk));
    for _ in 0..20 {
        if at_priority(&g) && g.stack().is_empty() {
            break;
        }
        if g.kind() == Some(DecisionKind::NameCard) {
            break;
        }
        let c = if at_priority(&g) { Choice::Pass } else { options(&g)[0] };
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}", log(&g));
    let mut want: Vec<u16> = vec![act::PASS];
    want.extend(act::LIBRARY..act::LIBRARY + 64);
    assert_eq!(actions(&g), want);
    let mut defs: Vec<engine::ids::DefId> = names.iter().map(|n| db().id(n).unwrap()).collect();
    defs.push(db().id("Anointed Peacekeeper").unwrap());
    defs.push(db().id("Plains").unwrap());
    defs.sort_unstable();
    defs.dedup();
    assert_eq!(defs.len(), 72);
    assert_eq!(options(&g)[1..64], defs[..63].iter().map(|&d| Choice::Def(d)).collect::<Vec<_>>()[..]);
    g.choose(next_page(&g));
    same_after_determinize(&g);
    assert_eq!(options(&g), defs[63..].iter().map(|&d| Choice::Def(d)).collect::<Vec<_>>());
    assert_eq!(actions(&g), (act::LIBRARY..act::LIBRARY + 9).collect::<Vec<_>>());
    let own = *defs[63..].iter().find(|&&d| names[30..].iter().any(|n| db().id(n) == Some(d))).expect("one of P0's names on page 2");
    g.choose(Choice::Def(own));
    assert!(g.log_lines().iter().any(|l| l.contains(&format!("chosen name: {}", db().def(own).name))), "{}", log(&g));
}
