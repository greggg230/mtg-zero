//! ENGINE.md 128: a hand card both players know, named by a hand slot. A hand slot is the
//! place of its definition among the whole hand, the hidden cards' definitions included
//! (ENGINE.md 11), so in two worlds of the other player's one action can name two different
//! cards it knows. Every such card that becomes visible is keyed by its identity (`reveal`,
//! as `move_card` does), so zero-core, which keeps worlds with equal keys, decider and legal
//! actions in one node, never keeps two worlds whose views differ. And the worlds are in the
//! normal form of every state (`canon_hand_knowledge` after the deal), and a slot takes a
//! twin an ability names that the other player knows first (`hand_slot_order_in`), so the
//! card a slot names is known to the other player in every world or in none.
//!
//! Tests: the two fuzz seeds that found it (`FUZZ_POOL=names FUZZ_HIDDEN_DECKS=1`, 466 and
//! 4248: each world's encoding differed after the same face choice with equal keys), a
//! minimal hand-built case, the canonical worlds, and the slot of a twin an ability names.

use engine::az::{Format, MtgAz, Seat};
use engine::db::CardDb;
use engine::hidden::{key_kind, key_payload, KEY_REVEAL};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, GameSpec, Step, Zone};
use zero_search::game::SearchGame;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn id(name: &str) -> u16 {
    db().expect_id(name).unwrap()
}

fn legal(a: &MtgAz) -> Vec<u16> {
    let mut l = Vec::new();
    a.legal_actions(&mut l);
    l
}

/// `viewer`'s whole row (layout v7, ENGINE.md 125: the head, the tail tokens and the pointer
/// overrides; one information set, equal rows).
fn enc(a: &MtgAz, viewer: u8) -> Vec<f32> {
    engine::az::tail_encode::Row::of_az(a, viewer as usize).flat()
}

/// The reveal keys' identities among `keys`.
fn revealed(keys: &[u64]) -> Vec<u64> {
    keys.iter().filter(|&&k| key_kind(k) == KEY_REVEAL).map(|&k| key_payload(k)).collect()
}

/// The two seeds' states (`tests/data/known_face_key.json`, written by the fuzzer with the
/// fix turned off): the game (`FUZZ_POOL=names`'s decks, hidden decklists, the table's prior,
/// as `play_hidden` makes it) after its actions, the viewer, the world seed `s` of the
/// information-set check (worlds `s` and `s ^ 0x5EED`) and the actions both worlds took, the
/// last one the hand slot whose face choice the old engine left unkeyed.
#[test]
fn fuzz_seeds_466_and_4248_a_known_cards_face_choice_keeps_the_worlds_apart() {
    let data: serde_json::Value = serde_json::from_str(include_str!("data/known_face_key.json")).unwrap();
    for case in data.as_array().unwrap() {
        let seed = case["seed"].as_u64().unwrap();
        let deck = |k: &str| -> Vec<u16> { case[k].as_array().unwrap().iter().map(|n| id(n.as_str().unwrap())).collect() };
        let ids = |k: &str| -> Vec<u16> { case[k].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u16).collect() };
        let viewer = case["viewer"].as_u64().unwrap() as u8;
        let s: u64 = case["s"].as_str().unwrap().parse().unwrap();
        let gspec = GameSpec { hidden_decks: true, hidden_decklists: true, ..GameSpec::default() };
        let seat = |deck: Vec<u16>| Seat { deck, sideboard: Vec::new(), first_picks: Vec::new(), elsewhere: Vec::new() };
        let prior: std::sync::Arc<dyn engine::az::hidden_decks::DeckPrior> = engine::az::hidden_decks::TablePrior::of(db());
        let mut a = MtgAz::from_seats(db(), gspec, Format::Bo1, &[seat(deck("deck0")), seat(deck("deck1"))], seed, [Some(prior.clone()), Some(prior)])
            .unwrap();
        a.m.deal.k = 32;
        let mut keys = Vec::new();
        for x in ids("actions") {
            a.step(x, &mut keys).unwrap();
        }
        let mut x = a.clone();
        x.determinize(viewer as usize, &mut zero_search::rng::Rng::new(s));
        let mut y = a.clone();
        y.determinize(viewer as usize, &mut zero_search::rng::Rng::new(s ^ 0x5EED));
        let path = ids("path");
        for (step, &act) in path.iter().enumerate() {
            // One node: the same decider and legal actions, and so the same view.
            assert_eq!((x.to_move(), legal(&x)), (y.to_move(), legal(&y)), "seed {seed} step {step}");
            assert_eq!(x.g.view_of(viewer), y.g.view_of(viewer), "seed {seed} step {step}");
            assert!(enc(&x, viewer) == enc(&y, viewer), "seed {seed} step {step}: same node, different encodings");
            let last = step + 1 == path.len();
            if last {
                // The other player's hand slot: a face choice of a card the viewer knows, a
                // different one in each world.
                let card = |w: &MtgAz| match w.g.pending_decision().unwrap().choice_of(act) {
                    Some(Choice::Faces(c, _)) => c,
                    other => panic!("seed {seed}: not a face choice: {other:?}"),
                };
                let (cx, cy) = (card(&x), card(&y));
                assert!(x.g.knows(viewer, cx) && y.g.knows(viewer, cy), "seed {seed}: cards the viewer knows");
                assert_ne!(x.g.obj(cx).def, y.g.obj(cy).def, "seed {seed}: two cards");
            }
            let (mut kx, mut ky) = (Vec::new(), Vec::new());
            x.step(act, &mut kx).unwrap();
            y.step(act, &mut ky).unwrap();
            if last {
                // Keyed by what each world's action did: the worlds are told apart.
                assert_eq!(x.g.pending_decision().unwrap().kind, DecisionKind::ChooseFace);
                assert_eq!(y.g.pending_decision().unwrap().kind, DecisionKind::ChooseFace);
                assert_ne!(kx, ky, "seed {seed}: the face choices are keyed");
                assert_ne!(revealed(&kx), revealed(&ky), "seed {seed}");
            } else {
                assert_eq!(kx, ky, "seed {seed} step {step}: the recorded path keeps one node");
            }
        }
    }
}

/// One public state, two worlds of P0's: P1's hand holds Brazen Borrower and Life // Death,
/// both known to P0 (P0 saw them), and a card P0 doesn't know, whose definition comes before
/// both in one world and after both in the other. Every card can be cast, so the legal
/// actions are the same; but hand slot `HAND` + 1 is Brazen Borrower in the first world and
/// Life // Death in the second (a choice of face for each). The face choice reveals a card
/// both players know: keyed by its identity, the worlds are told apart (CR 701.16; before
/// ENGINE.md 128 no key, and P0's encodings differed: the context's card).
#[test]
fn a_hand_slot_naming_a_known_card_is_keyed_by_its_identity() {
    const BORROWER: &str = "Brazen Borrower // Petty Theft";
    const LIFE_DEATH: &str = "Life // Death";
    let (kb, kl) = (id(BORROWER), id(LIFE_DEATH));
    let (lo, hi) = (kb.min(kl), kb.max(kl));
    // One-mana instants P1 can cast, one before both definitions, one after.
    let cands = ["Brainstorm", "Ponder", "Lightning Bolt", "Swords to Plowshares", "Spell Pierce", "Consider", "Opt", "Preordain"];
    let below = cands.iter().copied().find(|n| db().id(n).is_some_and(|d| d < lo)).expect("a cheap spell before both");
    let above = cands.iter().copied().find(|n| db().id(n).is_some_and(|d| d > hi)).expect("a cheap spell after both");
    let world = |unknown: &str, other: &str| -> Game {
        Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &["Island"; 8])
            .library(1, &[other, "Island", "Island", "Island", "Island"])
            .battlefield(0, &["Thraben Inspector"])
            .battlefield(1, &["Island", "Island", "Island", "Forest", "Swamp", "Swamp", "Mountain", "Plains"])
            .graveyard(1, &["Thraben Inspector"])
            .hand(1, &[unknown])
            .hand_known(1, &[BORROWER, LIFE_DEATH])
            .at(Step::Main1, 1)
            .start()
    };
    let (mut g1, mut g2) = (world(below, above), world(above, below));
    for g in [&g1, &g2] {
        assert_eq!(g.pending_decision().unwrap().player, 1, "P1 at priority");
    }
    let opts = |g: &Game| -> Vec<u16> { g.pending_decision().unwrap().options.iter().map(|o| o.action).collect() };
    assert_eq!(opts(&g1), opts(&g2), "the same legal actions");
    // (P0's whole rows, layout v7: ENGINE.md 125.)
    let (x1, x2) = (engine::az::tail_encode::Row::of(&g1, 0).flat(), engine::az::tail_encode::Row::of(&g2, 0).flat());
    assert!(x1 == x2, "one information set of P0's");
    let act = engine::act::HAND + 1;
    let face = |g: &Game| match g.pending_decision().unwrap().choice_of(act) {
        Some(Choice::Faces(c, _)) => c,
        other => panic!("not a face choice: {other:?}"),
    };
    let (c1, c2) = (face(&g1), face(&g2));
    assert_eq!((g1.obj(c1).def, g2.obj(c2).def), (lo, hi), "slot 1: the lower definition, then the higher");
    assert!(g1.knows(0, c1) && g2.knows(0, c2));
    let (mut k1, mut k2) = (Vec::new(), Vec::new());
    g1.apply(act, &mut k1).unwrap();
    g2.apply(act, &mut k2).unwrap();
    assert_eq!(g1.kind(), Some(DecisionKind::ChooseFace));
    assert_eq!(g2.kind(), Some(DecisionKind::ChooseFace));
    assert_eq!(revealed(&k1), vec![lo as u64], "keyed although P0 knew it");
    assert_eq!(revealed(&k2), vec![hi as u64]);
}

/// The worlds `determinize` deals are canonical (`canon_hand_knowledge`): P1's hand holds a
/// Lightning Bolt P0 knows and, at a lower card index, a card P0 doesn't know, which some
/// worlds deal as another Lightning Bolt; in every world the hand slot of Lightning Bolt
/// names a card P0 knows, and what P0 knows of the hand is the same multiset.
#[test]
fn determinized_worlds_put_the_known_twin_first() {
    let g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 8])
        .library(1, &["Lightning Bolt", "Lightning Bolt", "Lightning Bolt", "Island", "Island"])
        .hand(1, &["Island"])
        .hand_known(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = id("Lightning Bolt");
    let known = |g: &Game| -> Vec<u16> {
        let mut v: Vec<u16> = g.player(1).hand.iter().filter(|&&c| g.knows(0, c)).map(|&c| g.obj(c).def).collect();
        v.sort_unstable();
        v
    };
    let real = known(&g);
    assert_eq!(real, vec![bolt]);
    let mut twins = 0;
    for s in 0..200 {
        let mut w = g.clone();
        w.determinize(0, &mut engine::rng::Rng::new(s));
        assert_eq!(known(&w), real, "world {s}: P0 knows the same of the hand");
        let hand: Vec<CardIdx> = w.player(1).hand.clone();
        if hand.iter().all(|&c| w.obj(c).def == bolt) {
            twins += 1;
        }
        for (_, c) in w.hand_slots(1) {
            if w.obj(c).def == bolt {
                assert!(w.knows(0, c), "world {s}: the slot names the known Bolt");
            }
        }
    }
    assert!(twins > 0, "some worlds deal the unknown card as a second Bolt");
}

/// A twin an ability names keeps its own knowledge (`canon_hand_knowledge` leaves it): P0 drew
/// Metamorphosis Fanatic, its first draw, and revealed it for miracle (its trigger names it);
/// another Fanatic, which P1 doesn't know, sits at a lower card index in P0's hand. The hand
/// slot of Fanatic names the revealed one, known to P1 in every world P1 deals, not the twin.
#[test]
fn a_hand_slot_names_the_known_twin_an_ability_names() {
    const FANATIC: &str = "Metamorphosis Fanatic";
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &[FANATIC])
        .library(0, &[FANATIC, "Island", "Island"])
        .battlefield(0, &["Swamp", "Swamp"])
        .library(1, &["Island"; 8])
        .turn_number(3)
        .at(Step::Upkeep, 0)
        .start();
    let twin = g.card(0, FANATIC, Zone::Hand);
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Yes); // reveal it
    // Its trigger on the stack, naming it (CR 702.94a-b).
    g.drive_until(|g| g.kind() == Some(DecisionKind::Priority) && !g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::Priority));
    let drawn = g.player(0).hand.iter().copied().find(|&c| c != twin && g.obj(c).def == id(FANATIC)).expect("the drawn Fanatic");
    assert!(twin.get() < drawn.get(), "the twin first by card index");
    assert!(g.knows(1, drawn) && !g.knows(1, twin));
    let slot = g.hand_slots(0).into_iter().find(|&(_, c)| g.obj(c).def == id(FANATIC)).unwrap();
    assert_eq!(slot.1, drawn, "the slot names the revealed Fanatic");
    for s in 0..50 {
        let mut w = g.clone();
        w.determinize(1, &mut engine::rng::Rng::new(s));
        let ws = w.hand_slots(0).into_iter().find(|&(_, c)| w.obj(c).def == id(FANATIC)).unwrap();
        assert_eq!(ws.1, drawn, "world {s}: the same card");
    }
}
