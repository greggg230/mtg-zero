//! ENGINE.md 152: the zero-farm's fuzz failures of campaigns `eoe-fuzz-4b44b07` and
//! `delver-fuzz-4b44b07` (the fuzz build at mtg-zero 4b44b07; each seed plays the same game on
//! main 267ae603, action for action). Each seed's game is replayed here from its decks and
//! actions as the fuzzer played them (`data/farm_fuzz_seeds.json`, from the fuzzer's step trace),
//! and the check that failed runs again; beside each, a minimal case.
//!
//! * 152.1 Delver seed 100834725, same keys, different views: a pick from a looked-at pile took,
//!   among twins, the card with the lower index; worlds deal the indices to different places.
//! * 152.2 EoE seed 100019080, same keys, different views: in a controlled turn the controller
//!   learned the searched library card by card, keyed in definition order (its order unkeyed).
//! * 152.3 EoE seed 100000075, activate.rs:1176: a payment of no mana finished at once and left
//!   the answer to its decision (a source picked instead of the pool) to the cost's choices. The
//!   seed failed in a world of the fuzz's determinize check, which a replay of the game alone
//!   doesn't reach; its test is `pay.rs`'s unit test
//!   `a_payment_that_is_paid_takes_the_answer_to_its_own_decision` (the seed itself passes).

use engine::db::{AbilitySlot, CardDb};
use engine::rng::Rng;
use engine::testkit::Scenario;

use engine::{Choice, DecisionKind, Game, GameSpec, Step, Zone};

/// One recorded seed: its pool's card table, decks and seed (the fuzzer's `new_game`), the
/// actions the game took, and for an information-set failure the viewer, the world seed `s`
/// (worlds `s` and `s ^ 0x5EED`) and the actions the two worlds took.
struct Case {
    seed: u64,
    db: &'static CardDb,
    decks: [Vec<u16>; 2],
    actions: Vec<u16>,
    viewer: Option<u8>,
    s: Option<u64>,
    path: Vec<u16>,
}

fn case(pool: &str, seed: u64) -> Case {
    let data: serde_json::Value = serde_json::from_str(include_str!("data/farm_fuzz_seeds.json")).unwrap();
    let c = data.as_array().unwrap().iter().find(|c| c["pool"] == pool && c["seed"] == seed).expect("a recorded seed");
    let db = if pool == "eoe" { CardDb::eoe() } else { CardDb::constructed() };
    let deck = |k: &str| -> Vec<u16> { c[k].as_array().unwrap().iter().map(|n| db.expect_id(n.as_str().unwrap()).unwrap()).collect() };
    let ids = |k: &str| -> Vec<u16> { c[k].as_array().map_or(Vec::new(), |v| v.iter().map(|x| x.as_u64().unwrap() as u16).collect()) };
    Case {
        seed,
        db,
        decks: [deck("deck0"), deck("deck1")],
        actions: ids("actions"),
        viewer: c["viewer"].as_u64().map(|v| v as u8),
        s: c["s"].as_str().map(|s| s.parse().unwrap()),
        path: ids("path"),
    }
}

/// The fuzzer's game of `c` (`tests/fuzz.rs::new_game`) after its first `n` actions.
fn game(c: &Case, n: usize) -> Game {
    let mut g = Game::with_sideboards(c.db, GameSpec { log: true, ..GameSpec::default() }, [&c.decks[0], &c.decks[1]], [&[], &[]], c.seed).unwrap();
    let mut keys = Vec::new();
    for &a in &c.actions[..n] {
        g.apply(a, &mut keys).unwrap();
    }
    g
}

fn legal(g: &Game) -> Vec<u16> {
    let mut l = Vec::new();
    g.legal_actions(&mut l);
    l
}

/// The fuzzer's information-set check (`check_information_set`) on the recorded worlds and path:
/// while the keys, the next decider and the legal actions agree, the views agree. Returns the
/// step whose keys told the worlds apart, or None if the whole path kept them in one node.
fn walk(c: &Case) -> Option<usize> {
    let g = game(c, c.actions.len());
    let (viewer, s) = (c.viewer.unwrap(), c.s.unwrap());
    let mut a = g.clone();
    a.determinize(viewer, &mut Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut Rng::new(s ^ 0x5EED));
    for (step, &x) in c.path.iter().enumerate() {
        assert_eq!((a.to_move(), legal(&a)), (b.to_move(), legal(&b)), "seed {} step {step}: one node", c.seed);
        assert_eq!(a.view_of(viewer), b.view_of(viewer), "seed {} step {step}: same keys, different views", c.seed);
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.apply(x, &mut ka).unwrap();
        b.apply(x, &mut kb).unwrap();
        if ka != kb {
            return Some(step);
        }
    }
    if a.to_move() == b.to_move() && legal(&a) == legal(&b) {
        assert_eq!(a.view_of(viewer), b.view_of(viewer), "seed {} after the path: same keys, different views", c.seed);
    }
    None
}

/// Answer `g`'s decisions (the first option; priority passed) until `stop`, the keys collected.
fn drive(g: &mut Game, keys: &mut Vec<u64>, stop: impl Fn(&Game) -> bool) {
    for _ in 0..50 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("a decision");
        if d.kind == DecisionKind::Priority {
            keys.extend(g.pass());
        } else {
            let c = d.options[0].choice;
            keys.extend(g.choose(c));
        }
    }
    panic!("drive: no progress\n{}", g.log_lines().join("\n"));
}

// ---- 152.1: a pile's twins, by their place --------------------------------

/// Delver fuzz seed 100834725 (step 6 of an information-set walk of P1's): Ponder looked at
/// Dragon's Rage Channeler, Lightning Bolt, Dragon's Rage Channeler (keyed in that order), and
/// "put on top: the Channeler" took the Channeler with the lower card index: the third card in
/// one world, the top one in the other. Same keys, two libraries in P1's view. Now the twin
/// nearest the top in both: the walk keeps one node and one view to its end.
#[test]
fn delver_seed_100834725_a_ponder_twin_is_taken_by_its_place() {
    let c = case("delver", 100834725);
    assert_eq!(walk(&c), None, "the recorded walk keeps one node to its end (it reaches the pick)");
}

/// The minimal case of 152.1, as the fuzz's walk had it: in each of P0's own worlds P0 cracks
/// Flooded Strand (a search, then a shuffle: each world's library order, card indices and all,
/// is its own) and casts Ponder. In every world whose look saw Dragon's Rage Channeler,
/// Lightning Bolt, Dragon's Rage Channeler (the Channelers' indices come in either order), the
/// first card put back is the Channeler nearest the top, and every two such worlds (the same
/// keys) show P0 the same library.
#[test]
fn a_pile_pick_among_twins_takes_the_one_nearest_the_top_in_every_world() {
    let db = CardDb::constructed();
    let g = Scenario::new(db)
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Flooded Strand"])
        .hand(0, &["Ponder"])
        .library(0, &["Dragon's Rage Channeler", "Lightning Bolt", "Dragon's Rage Channeler", "Island", "Island", "Island", "Lightning Bolt", "Dragon's Rage Channeler"])
        .library(1, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let (drc, bolt) = (db.expect_id("Dragon's Rage Channeler").unwrap(), db.expect_id("Lightning Bolt").unwrap());
    let mut seen: Vec<(Vec<u64>, Vec<u64>)> = Vec::new();
    let (mut lower_on_top, mut lower_below) = (0, 0);
    for s in 0..800u64 {
        let mut w = g.clone();
        w.determinize(0, &mut Rng::new(s));
        let mut keys = w.choose(Choice::Ability(w.card(0, "Flooded Strand", Zone::Battlefield), AbilitySlot::Activated(0)));
        drive(&mut w, &mut keys, |w| w.kind() == Some(DecisionKind::Priority) && w.stack().is_empty() && w.turn().priority == 0);
        keys.extend(w.choose(Choice::Cast(w.card(0, "Ponder", Zone::Hand))));
        drive(&mut w, &mut keys, |w| w.kind() == Some(DecisionKind::PutOnTop));
        let lib = w.player(0).library.clone();
        let top: Vec<_> = lib.iter().rev().take(3).copied().collect();
        if top.iter().map(|&c| w.obj(c).def).collect::<Vec<_>>() != [drc, bolt, drc] {
            continue;
        }
        if top[0].get() < top[2].get() {
            lower_on_top += 1;
        } else {
            lower_below += 1;
        }
        let pick = w.offered().into_iter().find(|&o| matches!(o, Choice::Card(x) if w.obj(x).def == drc)).expect("the Channeler is offered");
        assert_eq!(pick, Choice::Card(top[0]), "world {s}: the Channeler nearest the top");
        keys.extend(w.choose(pick));
        seen.push((keys, w.view_of(0)));
    }
    assert!(lower_on_top > 0 && lower_below > 0, "worlds with the twins' indices in both orders ({lower_on_top}, {lower_below})");
    for (i, (k, v)) in seen.iter().enumerate() {
        for (k2, v2) in &seen[i + 1..] {
            if k == k2 {
                assert_eq!(v, v2, "equal keys, equal views");
            }
        }
    }
}

// ---- 152.2: a controlled search, keyed from the top ------------------------

/// EoE fuzz seed 100019080 (step 6 of an information-set walk of P1's, which controls P0's turn,
/// CR 722): P0's Lander searched P0's library, and P1, seeing what P0 sees (722.3), learned every
/// card of it where it is, keyed in definition order: two worlds whose libraries held the same
/// eleven cards in different orders had equal keys and different views. Now the keys list the
/// library from the top: these two worlds are told apart by the search's keys.
#[test]
fn eoe_seed_100019080_a_controlled_search_keys_the_library_from_the_top() {
    let c = case("eoe", 100019080);
    assert_eq!(walk(&c), Some(c.path.len() - 1), "the search's keys tell the two libraries apart");
}

// ---- 152.5: a bag's count through the feature transform -------------------------

/// The minimal case of 152.5's encoding bound (the exponential fuzz seeds 10000021, 2095, 2295 and
/// 2570: `check_bounded`, an identity's count in the opponent's battlefield bag 6760.5 > 256). A bag
/// counts every card of an identity at 0.25, and with no cap on permanents (ENGINE.md 26.11.6) the
/// count went past the bound unsquashed. 1,100 Forests of the opponent's: 275 before, now
/// `squash(275)`; every value of P0's row within `FEATURE_BOUND`.
#[test]
fn an_identitys_bag_count_stays_within_the_feature_bound() {
    use engine::az::encode::{squash, FEATURE_BOUND};
    let g = Scenario::new(CardDb::constructed())
        .library(0, &["Island"; 5])
        .library(1, &["Island"; 5])
        .battlefield(1, &["Forest"; 1100])
        .at(Step::Main1, 0)
        .start();
    let row = engine::az::tail_encode::Row::of(&g, 0);
    let vals: Vec<f32> = row.head.iter().chain(row.tail.iter().flat_map(|t| t.2.iter())).copied().collect();
    let max = vals.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(max <= FEATURE_BOUND, "the largest value {max}");
    assert!(vals.contains(&squash(275.0)), "the Forests' count, squashed");
}
