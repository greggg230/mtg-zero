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
        .map(|(i, _)| g.ident(CardIdx::new(i as u32)))
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

// ---- Hidden decklists (ENGINE.md 102) ----

/// Brainstorm puts back the Test Bear P1 knew in P0's hand, then P0's fetch land shuffles the
/// library: P1's cloud now covers P0's whole hand and library. With open decklists it is dropped
/// (it says nothing the decklist does not); with hidden decklists it is the record that the Bear
/// is still in P0's deck, and is kept (ENGINE.md 102.1).
fn bear_brainstormed_then_shuffled(decklists: bool) -> (Game, u16) {
    let mut g = Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.hidden_decks = decklists;
            s.hidden_decklists = decklists;
        })
        .battlefield(0, &["Island", "Polluted Delta"])
        .hand(0, &["Brainstorm", "Mountain"])
        .hand_known(0, &["Test Bear"])
        .library(0, &["Forest", "Plains", "Swamp", "Island", "Lightning Bolt", "Counterspell", "Forest", "Forest"])
        .library(1, &["Island"; 6])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    let bear_def = g.obj(bear).def;
    g.choose(Choice::Cast(g.card(0, "Brainstorm", Zone::Hand)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Card(g.card(0, "Mountain", Zone::Hand)));
    assert_eq!(g.clouds().len(), 1, "the Brainstorm cloud");
    g.choose(Choice::Ability(g.card(0, "Polluted Delta", Zone::Battlefield), engine::db::AbilitySlot::Activated(0)));
    for _ in 0..40 {
        if g.player(0).graveyard.iter().any(|&c| g.db().def(g.obj(c).def).name == "Polluted Delta") && g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = d.options.iter().find(|o| !matches!(o.choice, Choice::Pass)).map_or(d.options[0].choice, |o| o.choice);
        let c = if d.kind == DecisionKind::Priority { Choice::Pass } else { c };
        g.choose(c);
    }
    g.check_invariants().unwrap();
    g.check_knowledge().unwrap();
    (g, bear_def)
}

#[test]
fn hidden_decklists_keep_a_cloud_over_the_whole_hand_and_library() {
    let (open, _) = bear_brainstormed_then_shuffled(false);
    assert!(open.clouds().is_empty(), "open decklists: the cloud over hand and library says nothing: {:?}", open.clouds());
    let (g, bear) = bear_brainstormed_then_shuffled(true);
    assert_eq!(g.clouds().len(), 1, "{:?}", g.clouds());
    let k = &g.clouds()[0];
    assert_eq!((k.viewer, k.owner, k.hand, k.library), (1, 0, true, true));
    assert_eq!(k.defs, vec![bear]);
    // Every world of P1's keeps the Bear in P0's hand or library.
    for s in 0..100 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_invariants().unwrap();
        d.check_knowledge().unwrap();
        assert!(!slots_with(&d, 0, bear).is_empty(), "world {s}: the Bear left P0's hand and library");
    }
}

use engine::az::encode::{bag, glob, NUM_FEATURES, O_BAGS, O_GLOBALS};
use engine::az::hidden_decks::{check_sample, DeckPrior, DeckWant, TablePrior};
use engine::az::{MtgAz, MtgSpec};
use zero_search::game::SearchGame;

fn az_enc(a: &MtgAz, v: usize) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    a.encode(v, &mut x);
    x
}

fn az_legal(a: &MtgAz) -> Vec<u16> {
    let mut l = Vec::new();
    a.legal_actions(&mut l);
    l
}

/// Mid-game positions (turn 2 or later) of cube games with hidden decklists (random play; one
/// every ~`every` decisions), `n` of them.
fn hidden_positions(n: usize, every: u32, seed: u64) -> Vec<MtgAz> {
    let sp = MtgSpec::parse(r#"{"hidden_decklists": true}"#).unwrap();
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    let mut s = 0;
    while out.len() < n {
        let mut a = MtgAz::new_game(&sp, 2, seed * 1000 + s).unwrap();
        s += 1;
        let mut keys = Vec::new();
        while a.to_move().is_some() && out.len() < n {
            if a.g.turn().number >= 2 && rng.below(every) == 0 {
                out.push(a.clone());
            }
            let l = az_legal(&a);
            a.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
    }
    out
}

/// Player `p`'s cards in the game, by card (sorted): their starting deck's (`MtgAz::g`'s objects
/// are the two decks first, P0's then P1's).
fn deck_cards(a: &MtgAz, p: u8) -> Vec<u16> {
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if p == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> = r.map(|i| engine::az::sample::card_of(&a.g, CardIdx::new(i as u32))).collect();
    v.sort_unstable();
    v
}

/// The cards the viewer knows of `o`'s deck (every deck card it knows exactly, by card, and its
/// clouds' identities): what every world's deck of `o`'s must hold.
fn shown_of(a: &MtgAz, viewer: u8) -> Vec<u16> {
    let o = 1 - viewer;
    let g = &a.g;
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if o == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> = r.map(|i| CardIdx::new(i as u32)).filter(|&c| g.knows(viewer, c)).map(|c| engine::az::sample::card_of(g, c)).collect();
    for k in g.clouds().iter().filter(|k| k.viewer == viewer && k.owner == o) {
        v.extend(k.defs.iter().map(|&d| g.db().def(d).card_def(d)));
    }
    v.sort_unstable();
    v
}

fn holds(big: &[u16], small: &[u16]) -> bool {
    let mut b = big.to_vec();
    small.iter().all(|d| match b.binary_search(d) {
        Ok(i) => {
            b.remove(i);
            true
        }
        Err(_) => false,
    })
}

/// ENGINE.md 102.1's constraints, world by world: every card the viewer knows stays where it is,
/// the opponent's sampled deck has the public size and holds every card the viewer has seen (with
/// copies), the viewer's own cards are untouched; the viewer's view, legal actions (when it
/// decides) and whole encoding are those of the real game; and most worlds change which cards
/// make up the opponent's deck (the deal is not the real deck's).
#[test]
fn hidden_decklist_worlds_hold_what_the_viewer_saw_and_deal_the_rest() {
    let pos = hidden_positions(60, 30, 102);
    let mut zr = zero_search::rng::Rng::new(102);
    let (mut worlds, mut other_deck) = (0, 0);
    for a in &pos {
        let mover = a.to_move().unwrap() as u8;
        for viewer in [mover, 1 - mover] {
            let o = 1 - viewer;
            let shown = shown_of(a, viewer);
            let (real, mine, x, view) = (deck_cards(a, o), deck_cards(a, viewer), az_enc(a, viewer as usize), a.g.view_of(viewer));
            for _ in 0..6 {
                let mut d = a.clone();
                d.determinize(viewer as usize, &mut zr);
                d.g.check_invariants().unwrap();
                d.g.check_knowledge().unwrap();
                for (i, ob) in a.g.objs().iter().enumerate() {
                    let c = CardIdx::new(i as u32);
                    if a.g.knows(viewer, c) {
                        assert_eq!(ob.def, d.g.obj(c).def, "a card P{viewer} knows was resampled");
                    }
                }
                let world = deck_cards(&d, o);
                assert_eq!(world.len(), real.len(), "the deck's size");
                assert!(holds(&world, &shown), "the world's deck lacks a card P{viewer} saw");
                assert_eq!(deck_cards(&d, viewer), mine, "P{viewer}'s own cards");
                assert_eq!(d.m.deck(o), world, "the match's deck of P{o} is the world's");
                assert_eq!(d.g.view_of(viewer), view, "P{viewer}'s view");
                if viewer == mover {
                    assert_eq!(az_legal(&d), az_legal(a), "the decider's actions");
                }
                assert_eq!(az_enc(&d, viewer as usize), x, "P{viewer}'s encoding");
                worlds += 1;
                other_deck += (world != real) as u32;
            }
        }
    }
    println!("{} positions, {worlds} worlds, {other_deck} with another deck", pos.len());
    assert!(other_deck * 10 > worlds * 9, "the deal changes the deck: {other_deck} of {worlds}");
}

/// No encoding or option set tells the opponent's unseen list (ENGINE.md 102.1): the bags of
/// the opponent's unseen cards and of their pool, and their sideboard's size, are zero for both
/// viewers (with open decklists the same games fill them); extreme worlds (a tilt of 10^6 on one
/// card, so nearly every unseen card is that card) still encode as the real game and keep the
/// decider's actions.
#[test]
fn no_encoding_or_option_tells_an_unseen_decklist() {
    let pos = hidden_positions(40, 30, 1020);
    let mut zr = zero_search::rng::Rng::new(1020);
    let row = |x: &[f32], b: usize| x[O_BAGS + b * engine::az::encode::D..O_BAGS + (b + 1) * engine::az::encode::D].iter().any(|&f| f != 0.0);
    let mut extreme = 0;
    for a in &pos {
        let mover = a.to_move().unwrap();
        for v in 0..2usize {
            let x = az_enc(a, v);
            assert!(!row(&x, bag::OPP_HIDDEN) && !row(&x, bag::OPP_POOL), "P{v}: the opponent's unseen cards or pool are encoded");
            assert_eq!(x[O_GLOBALS + 64 + glob::SIDEBOARD], 0.0, "P{v}: the opponent's sideboard size is encoded");
            for &star in &[a.g.db().id("Lightning Bolt").unwrap(), a.g.db().id("Forest").unwrap()] {
                let mut d = a.clone();
                d.determinize_deck_tilted(v, &mut zr, &|c| if c == star { 1e6 } else { 1.0 });
                assert_eq!(az_enc(&d, v), x, "P{v}'s encoding in an extreme world");
                if v == mover {
                    assert_eq!(az_legal(&d), az_legal(a), "the decider's actions in an extreme world");
                }
                extreme += 1;
            }
        }
    }
    // The control: the same games with open decklists show the opponent's unseen cards and pool.
    let sp = MtgSpec::parse("{}").unwrap();
    let a = MtgAz::new_game(&sp, 2, 1020 * 1000).unwrap();
    let x = az_enc(&a, 0);
    assert!(row(&x, bag::OPP_HIDDEN) && row(&x, bag::OPP_POOL), "open decklists encode them");
    println!("{} positions, {extreme} extreme worlds", pos.len());
}

/// zero-core's child signature (ENGINE.md 10): two worlds a viewer deals with hidden decklists,
/// played with the same actions, stay indistinguishable to it - the same view and encoding -
/// while their keys, next decider and legal actions agree.
#[test]
fn hidden_decklist_worlds_stay_indistinguishable_while_keys_agree() {
    let pos = hidden_positions(60, 30, 1021);
    let mut rng = Rng::new(1021);
    let (mut steps, mut split) = (0, 0);
    for a in &pos {
        let v = a.to_move().unwrap();
        let mut x = a.clone();
        x.determinize(v, &mut zero_search::rng::Rng::new(rng.next_u64()));
        let mut y = a.clone();
        y.determinize(v, &mut zero_search::rng::Rng::new(rng.next_u64()));
        for _ in 0..25 {
            let (lx, ly) = (az_legal(&x), az_legal(&y));
            if x.to_move() != y.to_move() || lx != ly {
                assert!(!(x.to_move() == Some(v) && y.to_move() == Some(v)), "the viewer's options differ");
                split += 1;
                break;
            }
            assert_eq!(x.g.view_of(v as u8), y.g.view_of(v as u8), "same keys, different views");
            assert_eq!(az_enc(&x, v), az_enc(&y, v), "same keys, different encodings");
            if x.to_move().is_none() {
                break;
            }
            let act = lx[rng.below(lx.len() as u32) as usize];
            let (mut kx, mut ky) = (Vec::new(), Vec::new());
            x.step(act, &mut kx).unwrap();
            y.step(act, &mut ky).unwrap();
            steps += 1;
            if kx != ky {
                split += 1;
                break;
            }
        }
    }
    println!("{} positions, {steps} steps in step, {split} split by keys or options", pos.len());
    assert!(steps > 200, "{steps}");
}

/// The stub prior's contract (ENGINE.md 102.1): exactly n cards holding the shown ones, every
/// other card in the support; never a failure (n shown, more shown lands than its target); a
/// revealed companion's condition kept; a tilt of 1 everywhere is the untilted deal draw for draw;
/// a sharp tilt concentrates the fill.
#[test]
fn the_table_prior_meets_its_contract() {
    let db = engine::db::CardDb::eoe();
    let p = TablePrior::of(db);
    let mut rng = Rng::new(7);
    let forest = db.id("Forest").unwrap();
    let spells: Vec<u16> = p.support().iter().copied().filter(|&d| !db.def(d).is(engine::db::types::LAND)).collect();
    for case in 0..300u64 {
        let k = (case % 41) as usize;
        let mut shown: Vec<u16> = (0..k).map(|i| if case % 3 == 0 && i < 25 { forest } else { spells[rng.below(spells.len() as u32) as usize] }).collect();
        shown.sort_unstable();
        let want = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None };
        let s = p.sample(&want, &mut rng);
        check_sample(&*p, &want, &s).unwrap();
        let lands = s.deck.iter().filter(|&&d| db.def(d).is(engine::db::types::LAND)).count();
        let shown_lands = shown.iter().filter(|&&d| db.def(d).is(engine::db::types::LAND)).count();
        assert!(lands == 17.max(shown_lands).min(40 - (k - shown_lands)), "lands {lands} for {k} shown ({shown_lands} lands)");
    }
    // A tilt of 1 is the untilted deal, draw for draw.
    let shown = vec![forest; 3];
    let one = |_: u16| 1.0;
    let w1 = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: Some(&one) };
    let w0 = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None };
    assert_eq!(p.sample(&w1, &mut Rng::new(9)), p.sample(&w0, &mut Rng::new(9)));
    // A sharp tilt: the starred spell takes most of the fill; the expected fill is the baseline.
    let star = spells[spells.len() / 2];
    let sharp = |d: u16| if d == star { 1e4 } else { 1.0 };
    let ws = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: Some(&sharp) };
    let (mut tilted, mut plain) = (0usize, 0usize);
    for s in 0..200 {
        tilted += p.sample(&ws, &mut Rng::new(s)).deck.iter().filter(|&&d| d == star).count();
        plain += p.sample(&w0, &mut Rng::new(s)).deck.iter().filter(|&&d| d == star).count();
    }
    let e = p.expected_fill(&w0)[star as usize];
    assert!((plain as f64 / 200.0 - e).abs() < 0.1 + 4.0 * (e / 200.0).sqrt(), "plain {plain} vs expected {e}");
    assert!(tilted > 200 * 15, "tilted {tilted}");
    // A revealed companion (cube: Lurrus, every permanent of mana value 2 or less): kept.
    let cube = engine::db::CardDb::cube();
    let lurrus = cube.id("Lurrus of the Dream-Den").unwrap();
    let pc = TablePrior::of(cube);
    for s in 0..100 {
        let want = DeckWant { db: cube, n: 40, shown: &[], companion: Some(lurrus), tilt: None };
        let x = pc.sample(&want, &mut Rng::new(s));
        check_sample(&*pc, &want, &x).unwrap();
        assert_eq!(x.sideboard, vec![lurrus]);
    }
}

/// The spec switch (ENGINE.md 102.1): off by default; best-of-one only, not with constructed
/// decks; on, the game hides the decks (`hidden_decks`, `hidden_decklists`), each deck has its
/// prior and the names a player may name for the other's deck are its support.
#[test]
fn the_hidden_decklists_switch() {
    assert!(!MtgSpec::parse("{}").unwrap().hidden_decklists);
    assert!(MtgSpec::parse(r#"{"hidden_decklists": true, "format": "bo3"}"#).is_err());
    assert!(MtgSpec::parse(r#"{"hidden_decklists": 1}"#).is_err());
    let sp = MtgSpec::parse(r#"{"hidden_decklists": true}"#).unwrap();
    let a = MtgAz::new_game(&sp, 2, 5).unwrap();
    assert!(a.g.spec().hidden_decks && a.g.spec().hidden_decklists && a.hidden_decklists());
    for p in 0..2 {
        let prior = a.m.deck_priors[p].as_ref().unwrap();
        assert_eq!(prior.name(), "table");
        assert!(deck_cards(&a, p as u8).iter().all(|d| prior.support().binary_search(d).is_ok()));
    }
    let off = MtgAz::new_game(&MtgSpec::parse("{}").unwrap(), 2, 5).unwrap();
    assert!(!off.g.spec().hidden_decklists && off.m.deck_priors.iter().all(|p| p.is_none()));
    // The same game but for the switch: the same decks, the same first decision.
    assert_eq!(deck_cards(&off, 0), deck_cards(&a, 0));
    assert_eq!(deck_cards(&off, 1), deck_cards(&a, 1));
}

// ---- The sealed prior (ENGINE.md 102.6) ----

/// The sealed prior's contract on real builds of S2's sealed spec (ENGINE.md 102.6): for shown
/// sets R drawn from a real deck (0 to 40 of its cards), the sampled deck has 40 cards, holds R,
/// every card in the support, and its nonbasic cards and the sideboard make a pool of the
/// collation's size; mostly in R's colours; a tilt of 1 is the untilted deal draw for draw.
#[test]
fn the_sealed_prior_meets_its_contract() {
    use engine::az::hidden_decks::SealedPrior;
    use engine::az::sealed::{collation::Collation, SealedSpec};
    let db = engine::az::sealed::db();
    let basics = engine::az::pools::basic_ids(db);
    let spec = SealedSpec::parse(&serde_json::json!({"set": "eoe", "collation": "arena"})).unwrap();
    let prior = SealedPrior::new(Collation::Arena, 6, spec.builder);
    let mut rng = Rng::new(1026);
    let (mut covered, mut two) = (0, 0);
    for seed in 0..120u64 {
        let real = spec.seat(seed, 0).unwrap().0.deck;
        let k = (seed % 41) as usize;
        let mut shown: Vec<u16> = (0..k).map(|i| real[(i * 7 + seed as usize) % real.len()]).collect();
        shown.sort_unstable();
        // (a multiset inside the real deck: drop extra copies)
        let mut left = real.clone();
        shown.retain(|d| match left.iter().position(|x| x == d) {
            Some(i) => {
                left.remove(i);
                true
            }
            None => false,
        });
        let want = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None };
        let s = prior.sample(&want, &mut rng);
        check_sample(&prior, &want, &s).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let nonbasic = s.deck.iter().filter(|d| !basics.contains(d)).count();
        assert_eq!(nonbasic + s.sideboard.len(), 78, "seed {seed}: a pool of six Arena boosters");
        let cols = |v: &[u16]| v.iter().filter(|&&d| !db.def(d).is(engine::db::types::LAND)).fold(0u8, |m, &d| m | db.def(d).colors) & 31;
        // (R of two colours: the deck stays in them, a splash at most)
        if k >= 8 && cols(&shown).count_ones() == 2 {
            two += 1;
            covered += (cols(&s.deck).count_ones() <= 3) as u32;
        }
        let one = |_: u16| 1.0;
        let w1 = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: Some(&one) };
        assert_eq!(prior.sample(&w1, &mut Rng::new(seed)), prior.sample(&want, &mut Rng::new(seed)), "seed {seed}: a tilt of 1");
    }
    println!("{two} requests with 8+ shown cards of two colours, {covered} decks in those and a splash at most");
    assert!(two > 20 && covered * 10 >= two * 9, "{covered} of {two}");
}

/// S2's `"sealed"` spec hides the decklists (ENGINE.md 102.6), each seat's deck dealt from the
/// sealed prior: worlds hold what the viewer saw, keep its view, actions and encoding, and change
/// the deck; `"hidden_decklists": false` keeps S2's open decklists.
#[test]
fn the_sealed_spec_deals_hidden_decks_from_the_sealed_prior() {
    let open = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena"}, "hidden_decklists": false}"#).unwrap();
    assert!(!open.hidden_decklists);
    let sp = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "paper"}}"#).unwrap();
    assert!(sp.hidden_decklists);
    let mut rng = Rng::new(1027);
    let mut zr = zero_search::rng::Rng::new(1027);
    let (mut worlds, mut other) = (0, 0);
    for s in 0..6u64 {
        let mut a = MtgAz::new_game(&sp, 2, 1027 + s).unwrap();
        assert!(a.m.deck_priors.iter().all(|p| p.as_ref().is_some_and(|p| p.name() == "sealed")));
        let mut keys = Vec::new();
        while a.to_move().is_some() {
            if a.g.turn().number >= 3 && rng.below(60) == 0 {
                let v = a.to_move().unwrap() as u8;
                let (shown, real, x) = (shown_of(&a, v), deck_cards(&a, 1 - v), az_enc(&a, v as usize));
                for _ in 0..2 {
                    let mut d = a.clone();
                    d.determinize(v as usize, &mut zr);
                    let world = deck_cards(&d, 1 - v);
                    assert!(world.len() == real.len() && holds(&world, &shown), "game {s}: the world's deck");
                    assert_eq!(d.g.view_of(v), a.g.view_of(v));
                    assert_eq!(az_legal(&d), az_legal(&a));
                    assert_eq!(az_enc(&d, v as usize), x);
                    worlds += 1;
                    other += (world != real) as u32;
                }
            }
            let l = az_legal(&a);
            a.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
    }
    println!("{worlds} sealed worlds, {other} with another deck");
    assert!(worlds >= 20 && other == worlds, "{worlds} {other}");
}
