//! M7b planeswalkers, hidden information (ENGINE.md 10, 24): naming a card,
//! looking at another library's top card, searching for any number of
//! cards, casting during a resolution, reveals; each decision survives
//! `determinize` for its decider and is refreshed for the other player.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::hidden::{key_kind, key_player, KEY_REVEAL};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Activate loyalty ability `i` (targets by `answer`), then pass priority
/// until a decision other than priority is pending.
fn activate_to_decision(g: &mut Game, c: CardIdx, i: u8, answer: impl Fn(&Game) -> Option<Choice>) -> Vec<u64> {
    g.choose(Choice::Ability(c, AbilitySlot::Activated(i)));
    while !at_priority(g) {
        let ch = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(ch);
    }
    let mut keys = Vec::new();
    while at_priority(g) {
        keys.extend(g.pass());
    }
    keys
}

/// `determinize` for both viewers keeps each viewer's view; the decider's
/// legal actions stay; the other viewer's refresh asks the same kind of
/// decision.
fn check_determinize(g: &Game, seeds: u64) {
    let decider = g.to_move().unwrap();
    let kind = g.kind();
    let mut legal = Vec::new();
    g.legal_actions(&mut legal);
    for seed in 0..seeds {
        for viewer in [0u8, 1] {
            let mut d = g.clone();
            d.determinize(viewer, &mut Rng::new(seed * 7 + viewer as u64));
            assert_eq!(g.view_of(viewer), d.view_of(viewer), "viewer {viewer} seed {seed}");
            assert_eq!(d.kind(), kind);
            if viewer == decider {
                let mut l2 = Vec::new();
                d.legal_actions(&mut l2);
                assert_eq!(legal, l2, "the decider's options");
            }
            d.check_invariants().unwrap();
        }
    }
}

#[test]
fn tamiyo_names_a_card_of_her_library_a_decision_on_definitions() {
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Island", "Consider", "Test Bear", "Ponder", "Lightning Bolt", "Island"])
        .library(1, &["Forest"; 5])
        .hand(1, &["Forest", "Test Giant"])
        .battlefield(0, &["Tamiyo, Collector of Tales"])
        .at(Step::Main1, 0)
        .start();
    let t = bf(&g, 0, "Tamiyo, Collector of Tales");
    activate_to_decision(&mut g, t, 0, |_| None);
    assert_eq!(g.kind(), Some(DecisionKind::NameCard));
    check_determinize(&g, 6);
    // Naming is public; the four revealed cards are keyed for everyone.
    let bear = g.db().id("Test Bear").unwrap();
    let keys = g.choose(Choice::Def(bear));
    assert_eq!(keys.iter().filter(|&&k| key_kind(k) == KEY_REVEAL && key_player(k) == 0).count() >= 4, true);
    while !at_priority(&g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.count_in(0, "Test Bear", Zone::Hand), 1);
    assert_eq!(g.player(0).graveyard.len(), 3);
}

#[test]
fn jace_looks_at_an_opponents_top_card_the_decision_survives_determinize() {
    let mut g = manual()
        .library(0, &["Island"; 5])
        .library(1, &["Lightning Bolt", "Forest", "Test Giant", "Forest"])
        .hand(1, &["Test Bear"])
        .battlefield(0, &["Jace, the Mind Sculptor"])
        .at(Step::Main1, 0)
        .start();
    let j = bf(&g, 0, "Jace, the Mind Sculptor");
    activate_to_decision(&mut g, j, 0, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(Target::Player(1))));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    check_determinize(&g, 6);
}

#[test]
fn searching_for_any_number_of_cards_survives_determinize() {
    let mut g = manual()
        .library(0, &["Forest", "Island", "Forest", "Breeding Pool", "Island"])
        .library(1, &["Forest"; 5])
        .hand(1, &["Test Bear"])
        .battlefield(0, &["Nissa, Who Shakes the World"])
        .counters(0, "Nissa, Who Shakes the World", engine::db::Counter::Loyalty, 8)
        .at(Step::Main1, 0)
        .start();
    let n = bf(&g, 0, "Nissa, Who Shakes the World");
    activate_to_decision(&mut g, n, 1, |_| None);
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    check_determinize(&g, 6);
    // A set in non-decreasing definition order: Forest, then Forest again
    // (two copies); then no Breeding Pool (it sorts first).
    let forest = g.db().id("Forest").unwrap();
    let pool = g.db().id("Breeding Pool").unwrap();
    assert!(g.has_option(Choice::Def(pool)) && pool < forest);
    g.choose(Choice::Def(forest));
    assert!(g.has_option(Choice::Def(forest)), "a second copy");
    assert!(!g.has_option(Choice::Def(pool)));
    check_determinize(&g, 3);
    g.choose(Choice::Def(forest));
    g.choose(Choice::Pass);
    while !at_priority(&g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.lands_of(0).count(), 2);
}

#[test]
fn chandra_casting_during_the_resolution_survives_determinize() {
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .hand(1, &["Test Bear"])
        .battlefield(0, &["Chandra, Torch of Defiance", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let ch = bf(&g, 0, "Chandra, Torch of Defiance");
    let keys = activate_to_decision(&mut g, ch, 0, |_| None);
    assert!(keys.iter().any(|&k| key_kind(k) == KEY_REVEAL), "the exiled card is revealed");
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    check_determinize(&g, 6);
}

#[test]
fn karn_reveals_are_public_and_the_opponents_choice_survives_determinize() {
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Test Giant", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .hand(1, &["Test Bear"])
        .battlefield(0, &["Karn, Scion of Urza"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Karn, Scion of Urza");
    let keys = activate_to_decision(&mut g, k, 0, |_| None);
    assert_eq!(keys.iter().filter(|&&k| key_kind(k) == KEY_REVEAL).count(), 2);
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand));
    assert_eq!(g.to_move(), Some(1));
    check_determinize(&g, 6);
}
