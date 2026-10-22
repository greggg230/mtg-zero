//! Hidden information: determinize, knowledge, chance keys (ENGINE.md 10).

mod common;

use common::db;
use engine::game::StackKind;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DRAW};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, GameSpec, PlayerId, Step, Zone};

/// Everything a player can see regardless of viewer: public zones by
/// definition, sizes of hidden zones, life, turn.
fn public_view(g: &Game) -> Vec<u64> {
    let mut v = vec![g.turn().number as u64, g.turn().step as u64, g.life(0) as u64, g.life(1) as u64];
    for &c in g.battlefield() {
        v.push(g.obj(c).def as u64);
        v.push(g.obj(c).tapped as u64);
        v.push(g.obj(c).damage as u64);
    }
    for p in 0..2 {
        let pl = g.player(p);
        v.push(pl.hand.len() as u64);
        v.push(pl.library.len() as u64);
        v.extend(pl.graveyard.iter().map(|&c| g.obj(c).def as u64));
    }
    for s in g.stack() {
        if let StackKind::Spell { card } = s.kind {
            v.push(g.obj(card).def as u64);
        }
    }
    v
}

fn defs_of(g: &Game, owner: PlayerId) -> Vec<u16> {
    let mut d: Vec<u16> =
        g.objs().iter().filter(|o| o.owner == owner).map(|o| o.def).collect();
    d.sort();
    d
}

fn random_game(seed: u64, steps: usize) -> Game {
    let db = db();
    let mut rng = Rng::new(seed);
    let d0 = common::random_deck(db, &mut rng);
    let d1 = common::random_deck(db, &mut rng);
    let mut g = Game::new(db, GameSpec::default(), [&d0, &d1], seed).unwrap();
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    for _ in 0..steps {
        if g.is_over() {
            break;
        }
        g.legal_actions(&mut legal);
        g.apply(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
    }
    g
}

#[test]
fn determinize_changes_only_what_the_viewer_cannot_see() {
    let mut changed = 0;
    for seed in 0..40 {
        let g = random_game(seed, 60);
        let Some(viewer) = g.to_move() else { continue };
        let mut legal = Vec::new();
        g.legal_actions(&mut legal);
        let mut d = g.clone();
        d.determinize(viewer, &mut Rng::new(seed + 1000));
        let mut legal2 = Vec::new();
        d.legal_actions(&mut legal2);
        assert_eq!(legal, legal2, "seed {seed}: the viewer's legal actions changed");
        assert_eq!(public_view(&g), public_view(&d), "seed {seed}: public state changed");
        for p in 0..2 {
            assert_eq!(defs_of(&g, p), defs_of(&d, p), "seed {seed}: P{p}'s cards changed");
        }
        for &c in &g.player(viewer).hand {
            assert_eq!(g.obj(c).def, d.obj(c).def, "seed {seed}: the viewer's hand changed");
        }
        d.check_invariants().unwrap();
        let other = 1 - viewer;
        if g.player(other).hand.iter().any(|&c| g.obj(c).def != d.obj(c).def) {
            changed += 1;
        }
    }
    assert!(changed > 20, "the opponent's hand is resampled ({changed}/40)");
}

#[test]
fn known_cards_stay_in_place() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm", "Test Bear", "Mountain"])
        .library(0, &["Forest", "Plains", "Swamp", "Island", "Lightning Bolt", "Counterspell"])
        .library(1, &["Island"; 6])
        .at(Step::Main1, 0)
        .start();
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    let bear = g.card(0, "Test Bear", Zone::Hand);
    let mountain = g.card(0, "Mountain", Zone::Hand);
    g.choose(Choice::Card(mountain));
    g.choose(Choice::Card(bear));
    let top2: Vec<CardIdx> = g.player(0).library.iter().rev().take(2).copied().collect();
    let mut moved_for_opponent = false;
    for s in 0..30 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        for &c in &top2 {
            assert_eq!(d.obj(c).def, g.obj(c).def, "the owner knows the cards they put back");
        }
        let mut e = g.clone();
        e.determinize(1, &mut Rng::new(s));
        moved_for_opponent |= top2.iter().any(|&c| e.obj(c).def != g.obj(c).def);
    }
    assert!(moved_for_opponent, "the opponent does not know them");
}

#[test]
fn observer_masks_private_reveals() {
    let start = || {
        Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 4])
            .library(1, &["Forest", "Island", "Island"])
            .at(Step::End, 0)
            .start()
    };
    let draw_key = |g: &mut Game| -> u64 {
        let keys = g.drive_until(|g| g.turn().number == 4 && g.turn().step == Step::Main1);
        *keys.iter().find(|&&k| key_kind(k) == KEY_DRAW).expect("P1 draws")
    };
    // Without an observer the key names the card.
    let mut g = start();
    let k = draw_key(&mut g);
    assert_eq!((key_player(k), key_payload(k)), (1, db().id("Forest").unwrap() as u64));
    // Searching for P0: P1's draw is hidden from the observer.
    let mut g = start();
    g.determinize(0, &mut Rng::new(9));
    assert_eq!(key_payload(draw_key(&mut g)), HIDDEN);
    // Searching for P1: P1 sees its own draw.
    let mut g = start();
    g.determinize(1, &mut Rng::new(9));
    assert_ne!(key_payload(draw_key(&mut g)), HIDDEN);
}

#[test]
fn determinize_for_another_viewer_refreshes_the_pending_decision() {
    for seed in 0..40 {
        let g = random_game(seed, 40);
        let Some(mover) = g.to_move() else { continue };
        let mut d = g.clone();
        d.determinize(1 - mover, &mut Rng::new(seed));
        d.check_invariants().unwrap();
        let dec = d.pending_decision().expect("a decision");
        for o in &dec.options {
            match o.choice {
                Choice::Cast(c) | Choice::Play(c) => {
                    assert!(d.player(mover).hand.contains(&c));
                    let slots = d.hand_slots(mover);
                    assert!(slots.contains(&(o.action, c)), "seed {seed}: stale hand slot");
                }
                _ => {}
            }
        }
        // And the resampled world plays on.
        let mut legal = Vec::new();
        d.legal_actions(&mut legal);
        let mut keys = Vec::new();
        d.apply(legal[0], &mut keys).unwrap();
        d.check_invariants().unwrap();
    }
}

#[test]
fn cr_602_2a_an_ability_activated_from_hand_reveals_its_card() {
    // Cycling: the card stays in hand while its cost is paid, revealed, so
    // the opponent's determinize never changes it.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Raffine's Tower", "Test Bear", "Test Flyer"])
        .library(0, &["Forest"; 6])
        .library(1, &["Forest"; 6])
        .at(Step::Upkeep, 0)
        .start();
    let tower = g.card(0, "Raffine's Tower", Zone::Hand);
    g.choose(Choice::Ability(tower, engine::db::AbilitySlot::Hand(0)));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    assert!(g.knows(1, tower), "revealed to the opponent");
    for seed in 0..20 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(seed));
        assert_eq!(d.obj(tower).def, g.obj(tower).def);
        d.check_invariants().unwrap();
    }
}

#[test]
fn a_payment_in_progress_completes_in_every_determinized_world() {
    // P0 casts Test Double ({2}{R}) with two Islands and a Simian Spirit
    // Guide in hand. While the payment (or the cast before it) is in
    // progress, the opponent's determinize pins the payer's mana sources
    // in hand; a guide already picked is revealed, so public.
    let start = || {
        Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .battlefield(0, &["Island", "Island"])
            .hand(0, &["Simian Spirit Guide", "Test Double", "Test Bear", "Island"])
            .library(0, &["Forest", "Mountain", "Plains", "Swamp", "Lightning Bolt", "Counterspell"])
            .library(1, &["Forest"; 6])
            .at(Step::Main1, 0)
            .start()
    };
    let finish = |d: &mut Game| {
        let mut legal = Vec::new();
        for _ in 0..10 {
            if d.kind() == Some(DecisionKind::Priority) {
                break;
            }
            d.legal_actions(&mut legal);
            let mut keys = Vec::new();
            d.apply(legal[0], &mut keys).unwrap();
        }
        assert_eq!(d.kind(), Some(DecisionKind::Priority));
        assert_eq!(d.stack().len(), 1);
        assert_eq!(d.count_in(0, "Simian Spirit Guide", Zone::Exile), 1);
    };
    let mut g = start();
    let dbl = g.card(0, "Test Double", Zone::Hand);
    let guide = g.card(0, "Simian Spirit Guide", Zone::Hand);
    g.choose(Choice::Cast(dbl));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    // Before any pick: the guide is hidden from P1 but pinned.
    assert!(!g.knows(1, guide));
    let mut resampled = false;
    for seed in 0..30 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(seed));
        d.check_invariants().unwrap();
        assert_eq!(d.obj(guide).def, g.obj(guide).def, "a source the payment needs is pinned");
        resampled |= d.player(0).hand.iter().any(|&c| d.obj(c).def != g.obj(c).def);
        finish(&mut d);
    }
    assert!(resampled, "the rest of the payer's hand is still resampled");
    // After picking the guide: revealed, so every viewer knows it. (Sources
    // are picked in canonical class order: the Islands come first.)
    for _ in 0..2 {
        let island = g.pending_decision().unwrap().options[0].choice;
        assert!(matches!(island, Choice::Ability(_, engine::db::AbilitySlot::Activated(0))));
        g.choose(island);
    }
    assert!(!g.knows(1, guide));
    g.choose(Choice::Ability(guide, engine::db::AbilitySlot::Hand(0)));
    assert!(g.knows(1, guide));
    for seed in 0..10 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(seed));
        finish(&mut d);
    }
    // Once the payment is over nothing is pinned any more.
    let mut done = g.clone();
    done.drive_until(|g| g.kind() == Some(DecisionKind::Priority));
    let other = done.card(0, "Test Bear", Zone::Hand);
    let moved = (0..30).any(|s| {
        let mut d = done.clone();
        d.determinize(1, &mut Rng::new(s));
        d.obj(other).def != done.obj(other).def
    });
    assert!(moved);
}

#[test]
fn a_pending_shock_land_choice_survives_determinize() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &["Blood Crypt", "Test Bear", "Test Flyer"])
        .library(0, &["Forest"; 4])
        .library(1, &["Forest"; 4])
        .at(Step::Main1, 0)
        .start();
    let crypt = g.card(0, "Blood Crypt", Zone::Hand);
    g.choose(Choice::Play(crypt));
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    for viewer in 0..2 {
        let mut d = g.clone();
        d.determinize(viewer, &mut Rng::new(7));
        assert_eq!(d.obj(crypt).def, g.obj(crypt).def, "on the battlefield already: public");
        assert_eq!(d.kind(), Some(DecisionKind::YesNo));
        d.choose(Choice::Pass);
        assert!(d.obj(crypt).tapped);
    }
}
