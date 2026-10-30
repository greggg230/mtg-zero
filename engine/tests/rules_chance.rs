//! ENGINE.md 74: forced chance outcomes (zero-core's `chance_k`: an edge of
//! the search that has recorded `chance_k` outcomes replays one of them,
//! `Game::force_chance`, in the world walking it). The tests play the
//! search's two worlds - the one whose outcome was recorded and the one that
//! replays it - on real cube cards (and one the rules bug under the second
//! panic, CR 701.13a, alone); each fails on main f52c0bb, and the same way at
//! d0b62b1 and 6f871ac.

mod common;

use common::db;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Target, Zone};

/// P0 with Bolas's Citadel and the top card of its library a Test Bear,
/// `second` under it and `third` under that, and a Brainstorm in hand (so its
/// priority with the Bear on the stack is a decision whatever the top card
/// is); P1 with a nonland permanent (Petty Theft's target). P0 in its first
/// main phase.
fn citadel(second: &str, third: &str) -> Game {
    Scenario::new(db())
        .library(0, &["Test Bear", second, third, "Island", "Island"])
        .hand(0, &["Brainstorm"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Bolas's Citadel", "Island", "Island"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start()
}

/// The search transition of casting the top card with the Citadel: the
/// cast, then each decision of the cast answered by its first option, until
/// P0 has priority again. Returns the transition's chance keys.
fn cast_top(g: &mut Game) -> Vec<u64> {
    let top = *g.player(0).library.last().unwrap();
    let mut keys = g.choose(Choice::Cast(top));
    for _ in 0..20 {
        if g.kind() == Some(DecisionKind::Priority) {
            return keys;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        keys.extend(g.choose(c));
    }
    panic!("the cast does not end: {:?}", g.options_desc());
}

/// World `a` casts its top card and sees the next one (`seen`); world `b`,
/// whose next card is `other` (with `seen` under it), replays `a`'s keys.
/// -> (a, b) at P0's priority with the Test Bear on the stack.
fn replay_the_look(seen: &str, other: &str) -> (Game, Game) {
    let mut a = citadel(seen, other);
    let mut b = citadel(other, seen);
    let ka = cast_top(&mut a);
    b.force_chance(&ka);
    let kb = cast_top(&mut b);
    b.force_chance(&[]);
    assert_eq!(kb, ka, "the replay takes the look recorded in the other world");
    for g in [&a, &b] {
        let top = *g.player(0).library.last().unwrap();
        assert_eq!(g.name(top), seen, "{}", g.log_lines().join("\n"));
        assert!(g.knows(0, top) && !g.knows(1, top));
        g.check_invariants().unwrap();
    }
    (a, b)
}

/// The decision's actions.
fn legal(g: &Game) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// The h2h panics "a card with another face" (faces.rs, flow.rs): Priority
/// computed its options from the Citadel's new top card before its
/// controller looked at it, and the look's forced outcome then gave the card
/// another identity - the options named a Brazen Borrower's faces of what
/// was now a Lightning Bolt, at the recorded world's node (the same actions).
/// The search's walk there: the top card's action, then its other face; on
/// main that panics with the h2h's message. The look comes before the
/// options now: the action is a cast of the Bolt, which completes.
#[test]
fn a_forced_look_at_the_citadels_top_card_comes_before_the_options() {
    let (a, mut b) = replay_the_look("Lightning Bolt", "Brazen Borrower // Petty Theft");
    let top = *b.player(0).library.last().unwrap();
    assert_eq!(legal(&a), legal(&b), "{:?} / {:?}", a.options_desc(), b.options_desc());
    assert!(legal(&b).contains(&engine::act::LIBRARY), "{:?}", b.options_desc());
    assert_eq!(a.pending_decision().unwrap().choice_of(engine::act::LIBRARY), Some(Choice::Cast(*a.player(0).library.last().unwrap())));
    let choice = b.pending_decision().unwrap().choice_of(engine::act::LIBRARY);
    let mut keys = Vec::new();
    b.apply(engine::act::LIBRARY, &mut keys).unwrap();
    if b.kind() == Some(DecisionKind::ChooseFace) {
        b.choose(Choice::Number(1));
    }
    assert_eq!(choice, Some(Choice::Cast(top)), "the Bolt's cast, no face of the Borrower: {:?}", b.options_desc());
    b.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    b.choose(Choice::Target(Target::Player(1)));
    b.drive_until(|g| g.stack().is_empty() || g.is_over());
    assert_eq!(b.life(1), 17, "the Bolt\n{}", b.log_lines().join("\n"));
    assert_eq!(b.count_in(0, "Lightning Bolt", Zone::Graveyard), 1);
    assert_eq!(b.count_in(0, "Test Bear", Zone::Battlefield), 1);
}

/// The interactions fuzz's failure (FUZZ_CHANCE, seed 81: "no legal target
/// for a required target"): the stale option was a cast of the card that
/// had been there. Here a Lightning Bolt's cast offered for what the forced
/// look made a Forest (no land drop with the Bear on the stack): the
/// replaying world offers the top card nothing, as the recorded one does.
#[test]
fn a_forced_look_at_the_citadels_top_card_leaves_no_stale_cast() {
    let (a, b) = replay_the_look("Forest", "Lightning Bolt");
    let top = *b.player(0).library.last().unwrap();
    assert_eq!(legal(&a), legal(&b), "{:?} / {:?}", a.options_desc(), b.options_desc());
    let d = b.pending_decision().unwrap();
    assert!(!d.options.iter().any(|o| matches!(o.choice, Choice::Cast(c) | Choice::Play(c) if c == top)), "{:?}", b.options_desc());
}

/// P0's Barrowgoyf (2/3: an instant and a land in P1's graveyard) attacking,
/// P0's library Blightsteel Colossus, Test Bear, three Islands (top first);
/// the scenario's seed is the shuffles'. Driven to "you may mill that many"
/// (2, the combat damage).
fn barrowgoyf(seed: u64) -> Game {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .seed(seed)
        .library(0, &["Blightsteel Colossus", "Test Bear", "Island", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Barrowgoyf"])
        .graveyard(1, &["Island", "Lightning Bolt"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bg = g.card(0, "Barrowgoyf", Zone::Battlefield);
    g.choose(Choice::Card(bg));
    g.choose(Choice::Pass);
    for _ in 0..100 {
        match g.kind() {
            Some(DecisionKind::YesNo) => return g,
            Some(DecisionKind::Priority | DecisionKind::DeclareBlockers) => {
                g.choose(Choice::Pass);
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    panic!("no mill question: {:?}", g.options_desc());
}

/// CR 701.13a: a mill puts the top cards into the graveyard together.
/// Blightsteel Colossus ("if it would be put into a graveyard from anywhere,
/// reveal it and shuffle it into its owner's library instead") is shuffled in
/// after the action: the other card milled is the Bear under it, whatever
/// the shuffle does. On main the library was shuffled between the two, and
/// the second card came from the shuffled library (the Bear, an Island or the
/// Colossus again).
#[test]
fn a_mill_takes_the_top_cards_together_and_shuffles_blightsteel_in_after_cr_701_13a() {
    for seed in 0..24u64 {
        let mut g = barrowgoyf(seed);
        g.choose(Choice::Yes);
        let bear = g.card(0, "Test Bear", Zone::Graveyard);
        assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "seed {seed}");
        let opts = &g.pending_decision().unwrap().options;
        assert_eq!(opts.len(), 2, "seed {seed}: {:?}", g.options_desc());
        assert!(g.has_option(Choice::Card(bear)) && g.has_option(Choice::Pass), "seed {seed}: {:?}", g.options_desc());
        assert_eq!(g.count_in(0, "Blightsteel Colossus", Zone::Library), 1, "seed {seed}");
        assert_eq!(g.player(0).library.len(), 4, "seed {seed}: the Colossus and three Islands");
        assert_eq!(g.player(0).graveyard.len(), 1, "seed {seed}");
        g.check_invariants().unwrap();
        g.check_knowledge().unwrap();
    }
}

/// The h2h panic "ChooseGraveyardCard: duplicate action ids" (paired seed 7,
/// game 37: Barrowgoyf's mill of 7 with Blightsteel Colossus second): the
/// search's world replaying another world's mill (`force_chance`) found the
/// Colossus's card on top of the library shuffled mid-mill, the queued
/// reveal made it the creature the other world had milled there, and the
/// mill's pile - which kept the Colossus's card as milled though it went
/// back to the library - held that card twice: "a creature card from among
/// them" offered it twice. The mill is one action now (the Colossus stays
/// until it is over) and its pile holds the cards put into the graveyard:
/// every replay is the recorded world's mill, the Bear offered once.
#[test]
fn a_replayed_mill_offers_each_milled_card_once() {
    // (Every replay first: on main one of them panicked in the question's
    // `ask`, "duplicate action ids", as the h2h did.)
    let mut runs = Vec::new();
    for sa in 0..16u64 {
        for sb in 16..32u64 {
            let mut a = barrowgoyf(sa);
            let mut b = barrowgoyf(sb);
            let ka = a.choose(Choice::Yes);
            b.force_chance(&ka);
            let kb = b.choose(Choice::Yes);
            b.force_chance(&[]);
            runs.push((sa, sb, a, b, ka, kb));
        }
    }
    for (sa, sb, a, b, ka, kb) in &runs {
        assert_eq!(kb, ka, "worlds {sa} / {sb}");
        for g in [a, b] {
            assert_eq!(g.kind(), Some(DecisionKind::ChooseGraveyardCard), "worlds {sa} / {sb}");
            let bear = g.card(0, "Test Bear", Zone::Graveyard);
            assert_eq!(g.options_desc().len(), 2, "worlds {sa} / {sb}: {:?}", g.options_desc());
            assert!(g.has_option(Choice::Card(bear)), "worlds {sa} / {sb}: {:?}", g.options_desc());
            g.check_invariants().unwrap();
            g.check_knowledge().unwrap();
        }
    }
}

/// Found by this branch's fuzz (the Delver pool, seed 16246 under the `fuzz`
/// profile: "same keys, different views"): Urza's Bauble's "look at a card
/// at random in target player's hand" marked the card seen as known to the
/// looker even when the looker already knew another card of the hand with
/// that identity. A hand's cards are not told apart, so seeing a Lightning
/// Bolt beside the Bolt already known is not knowing two: in the looker's
/// worlds of one node (the same keys: "a Bolt was seen") the one known Bolt
/// was seen in some and the other Bolt in others, and the looker knew one
/// Bolt or two. Not a chance-node bug (no forced outcome): an older one in
/// the look's knowledge (ENGINE.md 71.1's code).
#[test]
fn a_random_look_at_a_twin_of_a_known_card_knows_no_more_than_one() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Urza's Bauble"])
        .hand_known(1, &["Lightning Bolt"])
        .hand(1, &["Lightning Bolt", "Counterspell"])
        .library(0, &["Island"; 10])
        .library(1, &["Lightning Bolt"; 10])
        .at(Step::Main1, 0)
        .start();
    let ub = g.card(0, "Urza's Bauble", Zone::Battlefield);
    g.choose(Choice::Ability(ub, engine::db::AbilitySlot::Activated(0)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority));
    let bolt = db().id("Lightning Bolt").unwrap();
    let mut seen: Vec<(Vec<u64>, Vec<u64>)> = Vec::new();
    let mut bolts_seen = 0;
    for s in 0..48u64 {
        // A world of the looker (its own generator), the look resolved.
        let mut r = g.clone();
        r.determinize(0, &mut engine::rng::Rng::new(500 + s));
        let mut keys = Vec::new();
        for _ in 0..20 {
            if r.stack().is_empty() && r.kind() == Some(DecisionKind::Priority) {
                break;
            }
            let a = if r.kind() == Some(DecisionKind::Priority) { engine::act::PASS } else { r.pending_decision().unwrap().options[0].action };
            r.apply(a, &mut keys).unwrap();
        }
        let known_bolts = r.player(1).hand.iter().filter(|&&c| r.knows(0, c) && r.obj(c).def == bolt).count();
        let saw_bolt = keys.iter().any(|&k| engine::hidden::key_kind(k) == engine::hidden::KEY_LOOK && engine::hidden::key_payload(k) == bolt as u64);
        if saw_bolt {
            bolts_seen += 1;
            assert_eq!(known_bolts, 1, "world {s}: a Bolt seen beside the known Bolt");
        }
        r.check_invariants().unwrap();
        r.check_knowledge().unwrap();
        seen.push((keys, r.view_of(0)));
    }
    assert!(bolts_seen > 4, "the worlds saw a Bolt: {bolts_seen}");
    for i in 0..seen.len() {
        for j in i + 1..seen.len() {
            if seen[i].0 == seen[j].0 {
                assert_eq!(seen[i].1, seen[j].1, "worlds {i} / {j}: same keys, different views of the looker");
            }
        }
    }
}
