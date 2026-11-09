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

/// The cards `owner` owns, by identity: a face-down permanent counts as the
/// card it is (`Game::ident`; determinize may give that identity to another
/// of the owner's hidden cards, ENGINE.md 31.2).
fn defs_of(g: &Game, owner: PlayerId) -> Vec<u16> {
    let mut d: Vec<u16> = g
        .objs()
        .iter()
        .enumerate()
        .filter(|(_, o)| o.owner == owner)
        .map(|(i, _)| g.ident(CardIdx(i as u16)))
        .collect();
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
                // (Cards in a graveyard or exile are public: only hand
                // slots can go stale.)
                Choice::Cast(c) | Choice::Play(c) if d.obj(c).zone != engine::Zone::Hand => {}
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

/// Where `card`'s identity sits after `determinize(viewer)`: the slot now
/// carrying the definition `def`, among `owner`'s hidden cards.
fn slots_with(d: &Game, owner: PlayerId, def: u16) -> Vec<CardIdx> {
    let pl = d.player(owner);
    pl.hand.iter().chain(pl.library.iter()).copied().filter(|&c| d.obj(c).def == def).collect()
}

#[test]
fn brainstorm_after_a_hand_reveal_leaves_partial_knowledge() {
    // P1 knew the Test Bear in P0's hand. P0 Brainstorms and puts two cards
    // back: P1 no longer knows where the Bear is, but knows it is in P0's
    // hand or among the two cards put back (not deep in the library).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm", "Mountain"])
        .hand_known(0, &["Test Bear"])
        .library(0, &["Forest", "Plains", "Swamp", "Island", "Lightning Bolt", "Counterspell", "Forest", "Forest"])
        .library(1, &["Island"; 6])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    let bear_def = g.obj(bear).def;
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Card(bear));
    let mountain = g.card(0, "Mountain", Zone::Hand);
    g.choose(Choice::Card(mountain));
    g.check_invariants().unwrap();
    assert!(!g.knows(1, bear), "P1 cannot tell which cards went back");
    assert_eq!(g.clouds().len(), 1);
    let k = &g.clouds()[0];
    assert_eq!((k.viewer, k.owner, k.hand, k.library), (1, 0, true, false));
    assert_eq!(k.defs, vec![bear_def]);
    assert_eq!(k.slots.len(), 2);
    let top2: Vec<CardIdx> = g.player(0).library.iter().rev().take(2).copied().collect();
    let (mut in_hand, mut on_top) = (0, 0);
    for s in 0..200 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_invariants().unwrap();
        let at = slots_with(&d, 0, bear_def);
        assert!(!at.is_empty(), "seed {s}: the Bear is somewhere");
        assert!(
            at.iter().any(|c| d.player(0).hand.contains(c) || top2.contains(c)),
            "seed {s}: the Bear left the hand and the top two"
        );
        in_hand += at.iter().any(|c| d.player(0).hand.contains(c)) as u32;
        on_top += at.iter().any(|c| top2.contains(c)) as u32;
    }
    assert!(in_hand > 20 && on_top > 20, "hand {in_hand}, top {on_top}");
    // P0 knows exactly.
    for s in 0..20 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        assert_eq!(d.obj(bear).def, bear_def);
    }
    // Drawing the top card keeps the knowledge exact for what P1 saw: the
    // drawn card joins the hand, which the cloud covers.
    g.drive_until(|g| g.turn().number == 5 && g.turn().step == Step::Main1);
    g.check_invariants().unwrap();
}

#[test]
fn a_card_leaving_the_hand_is_not_told_apart_from_a_known_twin() {
    // P1 knows one Lightning Bolt in P0's hand; P0 holds a second, unknown
    // to P1, and casts a Bolt. P1 cannot tell which one: it no longer knows
    // that P0 holds a Bolt.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt", "Island"])
        .hand_known(0, &["Lightning Bolt"])
        .library(0, &["Forest"; 6])
        .library(1, &["Forest"; 6])
        .at(Step::Main1, 0)
        .start();
    let bolts: Vec<CardIdx> =
        g.player(0).hand.iter().copied().filter(|&c| g.name(c) == "Lightning Bolt").collect();
    assert_eq!(bolts.iter().filter(|&&c| g.knows(1, c)).count(), 1);
    let pick = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .map(|o| o.choice)
        .find(|&c| matches!(c, Choice::Cast(x) if g.name(x) == "Lightning Bolt"))
        .unwrap();
    g.choose(pick);
    g.check_invariants().unwrap();
    let left = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.knows(1, left), "P1 does not know a Bolt is left");
    let bolt = db().id("Lightning Bolt").unwrap();
    let gone = (0..60).any(|s| {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.player(0).hand.iter().all(|&c| d.obj(c).def != bolt)
    });
    assert!(gone, "some sampled world has no Bolt in P0's hand");
}

#[test]
fn a_decision_the_engine_answers_itself_is_keyed() {
    // With auto_single on, P1's priority with nothing castable is answered
    // internally; that P1 had no alternative depends on its hidden hand, so
    // the transition carries a KEY_AUTO for it.
    use engine::hidden::KEY_AUTO;
    let start = |p1_hand: &[&str]| {
        Scenario::new(db())
            .spec(|s| s.auto_single = true)
            .battlefield(1, &["Island", "Island"])
            .hand(1, p1_hand)
            .library(0, &["Forest"; 6])
            .library(1, &["Forest"; 6])
            .at(Step::Main1, 0)
            .start()
    };
    let autos = |keys: &[u64]| keys.iter().filter(|&&k| key_kind(k) == KEY_AUTO && key_player(k) == 1).count();
    // P0 passes in main 1: P1 gets priority. With only lands in hand P1's
    // single option (pass) is answered for it. (Its Islands tapped: an
    // untapped one's mana ability is an option, `YES`, ENGINE.md 100.4.3.)
    let mut g = start(&["Forest"]);
    for c in g.battlefield().to_vec() {
        if g.obj(c).controller == 1 {
            g.set_tapped(c, true);
        }
    }
    let keys = g.pass();
    assert!(autos(&keys) >= 1, "{keys:?}");
    // With a castable instant P1 decides: no auto key for that decision.
    let mut g = start(&["Brainstorm"]);
    let keys = g.pass();
    assert_eq!(g.to_move(), Some(1));
    assert_eq!(autos(&keys), 0);
}

#[test]
fn casting_a_known_card_is_keyed() {
    // P1 knows P0's Lightning Bolt; casting it still emits its reveal key
    // (which known card a hand slot names differs between sampled worlds).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Mountain"])
        .hand_known(0, &["Lightning Bolt"])
        .library(0, &["Forest"; 6])
        .library(1, &["Forest"; 6])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let keys = g.choose(Choice::Cast(bolt));
    assert!(keys.iter().any(|&k| key_kind(k) == engine::hidden::KEY_REVEAL && key_payload(k) == g.obj(bolt).def as u64));
}

// ---- A non-deciding viewer's encoding (ENGINE.md 67) ------------------------

fn features_of(g: &Game, viewer: PlayerId) -> Vec<f32> {
    let mut x = vec![0.0; engine::az::encode::NUM_FEATURES];
    g.encode_features(viewer, &mut x);
    x
}

/// zero-core's root-view leaf values evaluate positions where the other
/// player decides from the root player's view, so a non-deciding viewer's
/// encoding must hold nothing of the decider's hidden cards - not even the
/// cards a payment in progress pins, which `determinize` keeps (ENGINE.md
/// 10) and so cannot test. `resample_unpinned` deals all of them again: the
/// viewer's features stay the same in every such world, during a payment
/// too. Random cube decks.
#[test]
fn a_non_deciding_viewers_features_ignore_the_deciders_hidden_cards() {
    let (mut worlds, mut changed, mut pinned) = (0, 0, 0);
    for seed in 0..60 {
        let g = random_game(seed, 20 + (seed as usize % 7) * 15);
        let Some(mover) = g.to_move() else { continue };
        let viewer = 1 - mover;
        let x = features_of(&g, viewer);
        pinned += !g.pinned_for(viewer).is_empty() as u32;
        for r in 0..4 {
            let mut d = g.clone();
            d.resample_unpinned(viewer, &mut Rng::new(seed * 31 + r));
            changed += g.player(mover).hand.iter().any(|&c| g.obj(c).def != d.obj(c).def) as u32;
            let y = features_of(&d, viewer);
            if let Some(i) = (0..x.len()).find(|&i| x[i] != y[i]) {
                panic!("seed {seed}: P{viewer}'s feature {i} reads P{mover}'s hidden cards ({} -> {})", x[i], y[i]);
            }
            worlds += 1;
        }
    }
    println!("{worlds} worlds, the decider's hand resampled in {changed}, {pinned} positions with pinned cards");
    assert!(worlds > 200 && changed > worlds / 2, "{worlds} {changed}");
}

/// The same while Force of Will's alternative cost is being paid (the blue
/// cards it may exile from P1's hand are pinned for P0) and while P1 pays
/// mana with a Simian Spirit Guide in hand: P0's features ignore P1's whole
/// hand, the pinned cards too, at every decision of the cast.
#[test]
fn a_non_deciding_viewers_features_ignore_pinned_cards_of_a_payment() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island"])
        .hand(0, &["Lightning Bolt", "Mountain"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Force of Will", "Brainstorm", "Counterspell", "Simian Spirit Guide", "Test Double", "Mountain"])
        .library(0, &["Island"; 6])
        .library(1, &["Forest", "Mountain", "Plains", "Swamp", "Lightning Bolt", "Counterspell"])
        .at(Step::Main1, 1)
        .start();
    let (mut checked, mut pinned_seen) = (0, 0);
    let mut check = |g: &Game, what: &str| {
        let Some(mover) = g.to_move() else { return };
        let viewer = 1 - mover;
        let x = features_of(g, viewer);
        let pins = g.pinned_for(viewer);
        pinned_seen += !pins.is_empty() as u32;
        let mut moved_pin = pins.is_empty();
        for s in 0..24 {
            let mut d = g.clone();
            d.resample_unpinned(viewer, &mut Rng::new(s));
            moved_pin |= pins.iter().any(|&c| d.obj(c).def != g.obj(c).def);
            assert_eq!(x, features_of(&d, viewer), "{what}: P{viewer}'s features read P{mover}'s hidden cards");
            checked += 1;
        }
        assert!(moved_pin, "{what}: the pinned cards were never dealt again");
    };
    // P1 casts Test Double ({2}{R}) with two Islands and the guide: the payment pins the guide
    let dbl = g.card(1, "Test Double", Zone::Hand);
    g.choose(Choice::Cast(dbl));
    for k in 0..6 {
        if g.kind() != Some(DecisionKind::PayMana) {
            break;
        }
        check(&g, &format!("paying for Test Double, step {k}"));
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) && g.to_move() == Some(1));
    // P0 answers with Lightning Bolt at the double? (Not needed: P1 holds priority.) P1 casts Force of
    // Will on its own spell by its alternative cost: exiling a blue card from hand
    if let Some(fow) = g.find_card(1, "Force of Will", Zone::Hand) {
        g.choose(Choice::Cast(fow));
        for k in 0..8 {
            let Some(kind) = g.kind() else { break };
            if kind == DecisionKind::Priority && g.stack().len() >= 2 {
                break;
            }
            check(&g, &format!("casting Force of Will, step {k} ({kind:?})"));
            let c = g.pending_decision().unwrap().options.last().unwrap().choice;
            g.choose(c);
        }
    }
    println!("{checked} worlds, {pinned_seen} decisions with pinned cards");
    assert!(pinned_seen >= 2, "{pinned_seen}");
}

// ---- Belief-weighted deals (ENGINE.md 69) ----------------------------------

/// P0 to act; P1 holds two cards P0 cannot see, its library three: the hidden
/// pool {Lightning Bolt 1, Island 3, Counterspell 1} for a hand of 2.
fn urn_game() -> Game {
    Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island"])
        .hand(0, &["Mountain"])
        .library(0, &["Forest", "Plains", "Swamp", "Island"])
        .hand(1, &["Lightning Bolt", "Island"])
        .library(1, &["Island", "Island", "Counterspell"])
        .at(Step::Main1, 0)
        .start()
}

/// P(the hand holds the multiset {x, y}) when 2 cards are drawn one at a time
/// without replacement from `counts`, each card with probability proportional
/// to `w` (Wallenius' urn).
fn wallenius2(counts: &[(u16, u32)], w: &dyn Fn(u16) -> f64) -> Vec<((u16, u16), f64)> {
    let mut out: Vec<((u16, u16), f64)> = Vec::new();
    let tot: f64 = counts.iter().map(|&(d, c)| c as f64 * w(d)).sum();
    for &(x, cx) in counts {
        let p1 = cx as f64 * w(x) / tot;
        let tot2 = tot - w(x);
        for &(y, cy) in counts {
            let left = if y == x { cy - 1 } else { cy };
            let p2 = left as f64 * w(y) / tot2;
            let key = if x <= y { (x, y) } else { (y, x) };
            match out.iter_mut().find(|e| e.0 == key) {
                Some(e) => e.1 += p1 * p2,
                None => out.push((key, p1 * p2)),
            }
        }
    }
    out
}

fn hand_pair(g: &Game, p: PlayerId) -> (u16, u16) {
    let mut h: Vec<u16> = g.player(p).hand.iter().map(|&c| g.ident(c)).collect();
    h.sort();
    (h[0], h[1])
}

/// `determinize_weighted` draws the opponent's unknown hand slots from its
/// hidden pool by sequential weighted sampling: the hand's composition follows
/// Wallenius' urn exactly (20k deals of a 5-card pool), weights all 1 give
/// `determinize`'s (hypergeometric) distribution, and every constraint holds:
/// the viewer's actions and view, its own hand, every owner's cards.
#[test]
fn a_weighted_deal_follows_the_wallenius_urn() {
    let g = urn_game();
    let id = |n: &str| db().expect_id(n).unwrap();
    let (bolt, island, cs) = (id("Lightning Bolt"), id("Island"), id("Counterspell"));
    let counts = [(bolt, 1), (island, 3), (cs, 1)];
    let mut legal = Vec::new();
    g.legal_actions(&mut legal);
    let n = 20_000;
    let weights: [&dyn Fn(u16) -> f64; 3] = [
        &|d| if d == bolt { 6.0 } else if d == cs { 1.0 } else { 0.25 },
        &|d| if d == island { 3.0 } else { 0.5 },
        &|_| 1.0,
    ];
    for (k, w) in weights.iter().enumerate() {
        let want = wallenius2(&counts, *w);
        let mut seen: std::collections::HashMap<(u16, u16), u32> = Default::default();
        let mut uni: std::collections::HashMap<(u16, u16), u32> = Default::default();
        for s in 0..n {
            let mut d = g.clone();
            d.determinize_weighted(0, &mut Rng::new(s), *w);
            *seen.entry(hand_pair(&d, 1)).or_default() += 1;
            if s < 400 {
                let mut l2 = Vec::new();
                d.legal_actions(&mut l2);
                assert_eq!(legal, l2, "the viewer's actions");
                assert_eq!(public_view(&g), public_view(&d));
                for p in 0..2 {
                    assert_eq!(defs_of(&g, p), defs_of(&d, p), "P{p}'s cards");
                }
                assert_eq!(g.player(0).hand.iter().map(|&c| g.obj(c).def).collect::<Vec<_>>(), d.player(0).hand.iter().map(|&c| d.obj(c).def).collect::<Vec<_>>());
                d.check_invariants().unwrap();
            }
            if k == 2 {
                let mut e = g.clone();
                e.determinize(0, &mut Rng::new(s + 1_000_000));
                *uni.entry(hand_pair(&e, 1)).or_default() += 1;
            }
        }
        for (key, p) in &want {
            let f = *seen.get(key).unwrap_or(&0) as f64 / n as f64;
            let se = (p * (1.0 - p) / n as f64).sqrt();
            assert!((f - p).abs() < 4.0 * se + 1e-3, "weights {k}: {key:?} dealt {f:.4}, Wallenius {p:.4}");
            if k == 2 {
                let fu = *uni.get(key).unwrap_or(&0) as f64 / n as f64;
                assert!((fu - p).abs() < 4.0 * se + 1e-3, "determinize: {key:?} dealt {fu:.4}, hypergeometric {p:.4}");
            }
        }
        assert_eq!(seen.values().sum::<u32>() as u64, n);
    }
}

/// A cloud over the hand (P1 saw a card of P0's hand, then P0 Brainstormed:
/// it is in P0's hand or among the two cards put back) keeps its card in its
/// domain whatever the weights, and the weights move it between the hand and
/// the top of the library.
#[test]
fn a_weighted_deal_keeps_a_clouds_card_in_its_domain() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm", "Mountain"])
        .hand_known(0, &["Test Bear"])
        .library(0, &["Forest", "Plains", "Swamp", "Island", "Lightning Bolt", "Counterspell", "Forest", "Forest"])
        .library(1, &["Island"; 6])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    let bear_def = g.obj(bear).def;
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Card(bear));
    let mountain = g.card(0, "Mountain", Zone::Hand);
    g.choose(Choice::Card(mountain));
    assert_eq!(g.clouds().len(), 1);
    let top2: Vec<CardIdx> = g.player(0).library.iter().rev().take(2).copied().collect();
    let mut in_hand = [0u32; 3];
    for (k, wb) in [1.0, 50.0, 0.02].into_iter().enumerate() {
        let w = move |d: u16| if d == bear_def { wb } else { 1.0 };
        for s in 0..600 {
            let mut d = g.clone();
            d.determinize_weighted(1, &mut Rng::new(s), &w);
            d.check_invariants().unwrap();
            d.check_knowledge().unwrap();
            for p in 0..2 {
                assert_eq!(defs_of(&g, p), defs_of(&d, p), "P{p}'s cards");
            }
            let at = slots_with(&d, 0, bear_def);
            assert!(at.iter().any(|c| d.player(0).hand.contains(c) || top2.contains(c)), "seed {s}: the Bear left its domain");
            in_hand[k] += at.iter().any(|c| d.player(0).hand.contains(c)) as u32;
        }
    }
    assert!(in_hand[1] > in_hand[0] + 60 && in_hand[2] + 60 < in_hand[0], "the Bear in hand: {in_hand:?} (weights 1, 50, 1/50)");
}

/// The deal's weights read only the viewer's information (ENGINE.md 69.2): in
/// worlds `determinize(viewer)` deals - which differ in the opponent's hidden
/// cards - the weights for one belief are the same, bit for bit.
#[test]
fn deal_weights_ignore_the_opponents_hidden_cards() {
    let (mut worlds, mut differ) = (0, 0);
    for seed in 0..40 {
        let g = random_game(seed, 20 + (seed as usize % 7) * 15);
        let Some(viewer) = g.to_move() else { continue };
        let mut r = Rng::new(seed);
        let p: Vec<f32> = (0..engine::az::identity::D).map(|_| r.below(1000) as f32 + 1.0).collect();
        let s: f32 = p.iter().sum();
        let p: Vec<f32> = p.iter().map(|x| x / s).collect();
        let w0 = engine::az::belief::deal_weights(&g, viewer, &p, 1.0, 20.0);
        for k in 0..4 {
            let mut d = g.clone();
            d.determinize(viewer, &mut Rng::new(seed * 10 + k));
            differ += (g.player(1 - viewer).hand.iter().any(|&c| g.ident(c) != d.ident(c))) as u32;
            assert_eq!(w0, engine::az::belief::deal_weights(&d, viewer, &p, 1.0, 20.0), "seed {seed}: the weights read a hidden card");
            worlds += 1;
        }
    }
    assert!(worlds > 100 && differ > worlds / 3, "{worlds} worlds, {differ} with another opponent hand");
}
