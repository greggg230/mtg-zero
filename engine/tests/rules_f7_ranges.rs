//! Workstream F7 of the faithfulness audit (docs/faithfulness_audit.md 5.7):
//! choice ranges and positions that were capped. "Look at the top X cards"
//! looks at exactly X (any X, the whole library when it holds fewer) and
//! pages the choice among them past the 64 list slots (`Game::ask_paged`,
//! ENGINE.md 100.1); "beneath the top X cards" puts a card at any depth. Each
//! test names the CR rule it checks.

mod common;

use engine::decision::act;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};

fn scen() -> Scenario {
    Scenario::new(common::db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    })
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

/// A determinized world of the decider asks the same decision (ENGINE.md 10,
/// 100.1: the page lives in the frame and the items are recomputed).
fn same_after_determinize(g: &Game) {
    let p = g.pending_decision().unwrap().player;
    for seed in [11, 12, 13] {
        let mut d = g.clone();
        d.determinize(p, &mut engine::rng::Rng::new(seed));
        assert_eq!(d.pending_decision(), g.pending_decision(), "determinize changed the decision");
    }
}

/// The definition with the lower id first: a pile sorted by definition puts
/// the second after every copy of the first (ENGINE.md 11).
fn by_id(a: &'static str, b: &'static str) -> (&'static str, &'static str) {
    let db = common::db();
    if db.expect_id(a).unwrap() < db.expect_id(b).unwrap() {
        (a, b)
    } else {
        (b, a)
    }
}

/// P0 casts Pillage the Bog ("Look at the top X cards of your library, where
/// X is twice the number of lands you control. Put one of them into your
/// hand and the rest on the bottom of your library in a random order.") with
/// `lands` lands and {B}{G} floating, over `library` (top first); the game at
/// the choice of the card to keep.
fn pillage(lands: usize, library: &[&str]) -> Game {
    let mut g = scen()
        .battlefield(0, &vec!["Bayou"; lands])
        .library(0, library)
        .library(1, &["Island"; 5])
        .hand(0, &["Pillage the Bog"])
        .pool(0, [0, 0, 1, 0, 1, 0], 0)
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Pillage the Bog", Zone::Hand);
    g.choose(Choice::Cast(c));
    for _ in 0..50 {
        if g.kind() == Some(DecisionKind::PutInHand) {
            return g;
        }
        if at_priority(&g) {
            g.pass();
        } else {
            let c = options(&g)[0];
            g.choose(c);
        }
    }
    panic!("no PutInHand decision\n{}", log(&g));
}

/// 31 lands: Pillage the Bog looks at the top 62 cards, and the 62nd may be
/// the one kept (CR 608.2d: the choice is among every card looked at).
/// Before, "the top X" was cut at 60 cards (docs/shortcuts.md 46.4) and the
/// 61st and 62nd were never seen.
#[test]
fn cr_608_2d_pillage_the_bog_looks_at_62_cards_and_may_keep_the_62nd() {
    let mut lib = vec!["Island"; 61];
    lib.push("Lightning Bolt");
    lib.extend(["Island"; 38]);
    let mut g = pillage(31, &lib);
    assert!(log(&g).contains("P0 looks at 62 card(s)"), "{}", log(&g));
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    assert!(g.has_option(Choice::Card(bolt)), "the 62nd card is offered: {:?}", g.options_desc());
    g.choose(Choice::Card(bolt));
    g.drive_until(|g| at_priority(g) && g.stack().is_empty());
    assert_eq!(g.obj(bolt).zone, Zone::Hand, "{}", log(&g));
    // The other 61 on the bottom (in a random order): 38 cards above them.
    assert_eq!(g.player(0).library.len(), 99);
}

/// 35 lands: Pillage the Bog looks at the top 70 cards, 69 copies of one
/// card and, 70th, a card sorted after them: its list position is 69, past
/// the 64 `LIBRARY` slots, so the choice pages (ENGINE.md 100.1). The first
/// page offers the copies at `LIBRARY` + 0 and "next page" at `LIBRARY` + 63;
/// the next page offers the 70th card alone at `LIBRARY` + 6 (69 - 63), which
/// points at its list token, place 69 (layout v7, ENGINE.md 125: the whole
/// list, a tail token past the 64 head slots); it is kept (CR 608.2d).
/// Each page is asked again identically in the decider's determinized worlds.
#[test]
fn cr_608_2d_pillage_the_bog_pages_70_cards_and_may_keep_the_70th() {
    let (filler, last) = by_id("Island", "Lightning Bolt");
    let mut lib = vec![filler; 69];
    lib.push(last);
    lib.extend([filler; 10]);
    let mut g = pillage(35, &lib);
    assert!(log(&g).contains("P0 looks at 70 card(s)"), "{}", log(&g));
    let target = g.card(0, last, Zone::Library);
    assert_eq!(actions(&g), vec![act::LIBRARY, act::LIBRARY + 63], "{:?}", g.options_desc());
    assert!(matches!(options(&g)[1], Choice::Page(_)));
    same_after_determinize(&g);
    let next = options(&g)[1];
    g.choose(next);
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand));
    assert_eq!(actions(&g), vec![act::LIBRARY + 6]);
    assert_eq!(options(&g), vec![Choice::Card(target)]);
    same_after_determinize(&g);
    // The other player's worlds deal the looked-at cards again (hidden from
    // them): P0's choice is still pending there, on this page or, where it
    // holds no card, the first (ENGINE.md 100.1).
    for seed in [5, 6, 7] {
        let mut d = g.clone();
        d.determinize(1, &mut engine::rng::Rng::new(seed));
        assert_eq!((d.kind(), d.pending_decision().unwrap().player), (Some(DecisionKind::PutInHand), 0));
    }
    // The decider's row: the whole list of 70, the page's option pointing at
    // the 70th card's token (layout v7, ENGINE.md 125).
    {
        use engine::az::encode::{list, CODE};
        use engine::az::tail_encode::{check_pointers, code_identity, seg, Row};
        let row = Row::of(&g, 0);
        check_pointers(&g, 0, &row).unwrap();
        assert_eq!(row.tokens(seg::LIST).len(), 70);
        assert_eq!(row.pointer(0, act::LIBRARY + 6), Some((seg::LIST, 0, 69)));
        let ids = engine::az::identity::identities(g.db());
        let s = row.token(seg::LIST, 69).unwrap();
        assert_eq!(code_identity(s), Some(ids[g.obj(target).def as usize] as usize), "list token 69 holds the 70th card");
        assert_eq!((s[CODE + list::PRESENT], s[CODE + list::OPTION]), (1.0, 1.0));
        assert_eq!(row.token(seg::LIST, 68).unwrap()[CODE + list::OPTION], 0.0, "the 69th: not on this page");
    }
    g.choose(Choice::Card(target));
    g.drive_until(|g| at_priority(g) && g.stack().is_empty());
    assert_eq!(g.obj(target).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.player(0).library.len(), 79);
}

/// Unexpectedly Absent ("Put target nonland permanent into its owner's
/// library just beneath the top X cards of that library") with X = 300 over
/// a 400-card library: exactly 300 cards above it (CR 401.4; before, a depth
/// past 255 was taken as 255, docs/shortcuts.md 46.4). X is set on the spell
/// on the stack: a payment reaches no X past 255 yet (the u8 generic mana of
/// a cost, docs/shortcuts.md 46.3).
#[test]
fn cr_401_4_unexpectedly_absent_with_x_300_puts_it_beneath_the_top_300_of_400() {
    let mut g = scen()
        .battlefield(1, &["Test Bear"])
        .library(0, &["Island"; 5])
        .library(1, &vec!["Forest"; 400])
        .hand(0, &["Unexpectedly Absent"])
        .pool(0, [2, 0, 0, 0, 0, 0], 0)
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    let ua = g.card(0, "Unexpectedly Absent", Zone::Hand);
    g.choose(Choice::Cast(ua));
    for _ in 0..20 {
        if at_priority(&g) {
            break;
        }
        let c = match g.kind() {
            Some(DecisionKind::ChooseMode) => Choice::Number(0),
            Some(DecisionKind::Target) => g.tgt(bear),
            _ => options(&g)[0],
        };
        g.choose(c);
    }
    assert_eq!(g.stack().len(), 1, "{}", log(&g));
    g.set_top_x(300);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let l: &[CardIdx] = &g.player(1).library;
    assert_eq!(l.len(), 401, "{}", log(&g));
    // (Index 0 = the bottom.)
    assert_eq!(l.iter().position(|&c| c == bear), Some(401 - 1 - 300), "300 cards above it\n{}", log(&g));
}
