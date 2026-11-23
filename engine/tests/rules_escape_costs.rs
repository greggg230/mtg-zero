//! Costs that read the graveyard, paid whole (CR 601.2h; ENGINE.md 247): the fuzz finds of the farm's payment lane,
//! each as a scenario. Every option a cast offers must lead to a completed cast, whatever is chosen after it: the tests
//! walk every decision of the cast every way.

use engine::db::CardDb;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Zone};

fn cube() -> Scenario {
    Scenario::new(CardDb::cube()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

/// Every state at the next priority from `g`'s pending decision, each decision answered every way (at most `limit`
/// states; a decision with no option panics in the engine).
fn every_way(g: &Game, limit: usize) -> Vec<Game> {
    let mut stack = vec![g.clone()];
    let mut out = Vec::new();
    while let Some(x) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        if x.is_over() || at_priority(&x) {
            out.push(x);
            continue;
        }
        for c in x.offered() {
            let mut y = x.clone();
            y.choose(c);
            stack.push(y);
        }
    }
    out
}

/// CR 601.2h, 702.138a (ruling bea302d9: a card the cost itself puts into the graveyard may be exiled for it): Tinker
/// ("as an additional cost to cast this spell, sacrifice an artifact") escaped by Underworld Breach with two other
/// cards in the graveyard and a Lotus Petal the only artifact - the Petal sacrificed for Tinker is the third card. The
/// payment activated the Petal's own mana ability first "so that it is in the graveyard" (`exile_sources`, which
/// counted the cost's discards but not its sacrifices), leaving Tinker no artifact: "picks within the budget" (fuzz
/// seed 100061712, ENGINE.md 247.2).
#[test]
fn cr_601_2h_702_138a_the_costs_own_sacrifice_feeds_the_escape_not_a_mana_source_first() {
    let g = cube()
        .battlefield(0, &["Underworld Breach", "Lotus Petal", "Island", "Island", "Island"])
        .graveyard(0, &["Tinker", "Ponder", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let tinker = g.card(0, "Tinker", Zone::Graveyard);
    let petal = g.card(0, "Lotus Petal", Zone::Battlefield);
    assert!(g.has_option(Choice::Cast(tinker)), "{}", opts(&g));
    let mut h = g.clone();
    h.choose(Choice::Cast(tinker));
    let ends = every_way(&h, 500);
    assert!(!ends.is_empty());
    for e in &ends {
        assert_eq!(e.obj(tinker).zone, Zone::Stack, "{}", log(e));
        assert_eq!(e.obj(petal).zone, Zone::Exile, "the Petal sacrificed for Tinker, then exiled by escape: {}", log(e));
    }
}
