//! Game setup and the London mulligan (CR 103).

mod common;

use common::db;
use engine::{Choice, DecisionKind, Game, GameSpec, Zone};

fn deck() -> Vec<u16> {
    let db = db();
    let names = ["Island", "Mountain", "Lightning Bolt", "Counterspell", "Test Bear"];
    (0..40).map(|i| db.id(names[i % names.len()]).unwrap()).collect()
}

#[test]
fn cr_103_setup_shuffles_and_deals_seven() {
    let d = deck();
    let mut g = Game::new(db(), GameSpec::default(), [&d, &d], 11).unwrap();
    // CR 103.1: first the play-or-draw choice, before the hands (35).
    assert_eq!(g.kind(), Some(DecisionKind::PlayDraw));
    assert!(g.player(0).hand.is_empty() && g.player(1).hand.is_empty());
    g.play_first();
    assert_eq!(g.turn().starting, g.turn().chooser);
    for p in 0..2 {
        assert_eq!(g.player(p).hand.len(), 7);
        assert_eq!(g.player(p).library.len(), 33);
    }
    // The first decision: the starting player's mulligan (103.5).
    assert_eq!(g.kind(), Some(DecisionKind::Mulligan));
    assert_eq!(g.to_move(), Some(g.turn().starting));
    g.check_invariants().unwrap();
    // Different seeds give different shuffles.
    let mut h = Game::new(db(), GameSpec::default(), [&d, &d], 12).unwrap();
    h.play_first();
    assert_ne!(g.fingerprint(), h.fingerprint());
}

#[test]
fn cr_103_5_london_mulligan() {
    let d = deck();
    let mut g = Game::new(db(), GameSpec::default(), [&d, &d], 11).unwrap();
    g.play_first();
    let s = g.turn().starting;
    let o = 1 - s;
    g.choose(Choice::Yes); // the starting player mulligans
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Mulligan), Some(o)));
    let keys = g.choose(Choice::Pass); // the other keeps
    // The mulligan happened: seven new cards (keyed draws), then one to the bottom.
    assert_eq!(keys.len(), 7);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::MulliganBottom), Some(s)));
    assert_eq!(g.player(s).hand.len(), 7);
    let pick = match g.pending_decision().unwrap().options[0].choice {
        Choice::Card(c) => c,
        other => panic!("{other:?}"),
    };
    g.choose(Choice::Card(pick));
    assert_eq!(g.player(s).hand.len(), 6);
    assert_eq!(g.player(s).library[0], pick, "on the bottom");
    assert_eq!(g.obj(pick).zone, Zone::Library);
    // Next round: only the starting player still decides.
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Mulligan), Some(s)));
    g.choose(Choice::Yes);
    for _ in 0..2 {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
    assert_eq!(g.player(s).hand.len(), 5);
    g.choose(Choice::Pass); // keep five
    assert_eq!(g.turn().number, 1);
    assert_eq!(g.player(s).mulligans, 2);
    assert_eq!(g.player(o).hand.len(), 7);
    g.check_invariants().unwrap();
}
