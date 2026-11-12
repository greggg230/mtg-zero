//! Constructed best-of-three (ENGINE.md 120): the Legacy Izzet Delver mirror's matches with
//! sideboarding. The spec (`"constructed": true` with `"format": "bo3"`: the main exactly 60, the
//! full Delver 75 since its five sideboard cards are supported (ENGINE.md 122), a 75 with a card the
//! table does not know refused, every such card named), the pool (exactly
//! the 75, no free basic land entries, CR 100.4a), sideboarding to exactly 60 (every legal 60
//! reachable in exactly one decision sequence), the information model (game 1 open, the later
//! games' 60s hidden within the public 75s and dealt by `ConstructedPrior` from what the viewer
//! knows), the prior's contract (its base prior and its exact conditioning), and the heuristic.

use std::collections::BTreeSet;

use engine::az::bots::Heuristic;
use engine::az::constructed::Decklist;
use engine::az::hidden_decks::{check_sample, ConstructedPrior, DeckPrior, DeckWant};
use engine::az::matches::{MatchPool, Sideboarding};
use engine::az::{DeckMode, MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::act;
use engine::rng::Rng;
use engine::DecisionKind;
use zero_search::game::SearchGame;

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn delver() -> Decklist {
    Decklist::izzet_delver()
}

/// Eight of the 75's sideboard cards, all from the cube (a 68 of the Delver 75: the smaller 75 these tests
/// play; the smoke's sideboard before the five outside the cube were supported, ENGINE.md 120.1, 129.2).
const SMOKE_SIDEBOARD: [(u32, &str); 4] = [(3, "Force of Negation"), (1, "Karakas"), (3, "Pyroblast"), (1, "Unholy Heat")];

fn names(list: &[(u32, &str)]) -> Vec<String> {
    list.iter().flat_map(|&(n, s)| std::iter::repeat_n(s.to_string(), n as usize)).collect()
}

/// The constructed best-of-three spec with both seats the Delver main and `side` (JSON).
fn bo3_json(side: &[String], max_turns: u32) -> String {
    let main = delver().main_names();
    serde_json::json!({"max_turns": max_turns, "constructed": true, "format": "bo3",
        "decks": {"p0": main, "p1": main, "p0_sideboard": side, "p1_sideboard": side}})
    .to_string()
}

fn smoke(max_turns: u32) -> MtgSpec {
    MtgSpec::parse(&bo3_json(&names(&SMOKE_SIDEBOARD), max_turns)).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// `viewer`'s whole row (layout v7, ENGINE.md 125: the head, the tail tokens - a later game's
/// `my_revealed` among them - and the pointer overrides; equal views, equal rows).
fn enc(g: &MtgAz, viewer: usize) -> Vec<f32> {
    engine::az::tail_encode::Row::of_az(g, viewer).flat()
}

/// Heuristic and random play mixed (the heuristic's sideboarding is `PASS`; random's a walk).
fn mixed(g: &MtgAz, h: &mut Heuristic, rng: &mut Rng) -> u16 {
    let l = legal(g);
    if g.sideboarding().is_none() && rng.below(3) != 0 {
        let a = h.choose(&g.g, &l);
        assert!(l.contains(&a));
        return a;
    }
    l[rng.below(l.len() as u32) as usize]
}

/// The cards `viewer` has seen of the opponent's current deck (deck cards it knows exactly, its
/// clouds' identities), sorted: every world's deck holds them (the fuzz's `hd_shown`).
fn shown(a: &MtgAz, viewer: u8) -> Vec<u16> {
    let o = 1 - viewer;
    let g = &a.g;
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if o == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> =
        r.clone().map(|i| engine::CardIdx::new(i as u32)).filter(|&c| g.knows(viewer, c)).map(|c| engine::az::sample::card_of(g, c)).collect();
    for k in g.clouds().iter().filter(|k| k.viewer == viewer && k.owner == o) {
        v.extend(k.defs.iter().map(|&d| g.db().def(d).card_def(d)));
    }
    v.sort_unstable();
    v
}

/// Player `p`'s cards of the current game's starting deck (sorted definitions).
fn game_deck(a: &MtgAz, p: u8) -> Vec<u16> {
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if p == 0 { 0..n0 } else { n0..n0 + n1 };
    let mut v: Vec<u16> = r.map(|i| engine::az::sample::card_of(&a.g, engine::CardIdx::new(i as u32))).collect();
    v.sort_unstable();
    v
}

/// Does sorted multiset `a` hold sorted multiset `b`?
fn holds(a: &[u16], b: &[u16]) -> bool {
    let mut left = a.to_vec();
    b.iter().all(|x| match left.binary_search(x) {
        Ok(i) => {
            left.remove(i);
            true
        }
        Err(_) => false,
    })
}

#[test]
fn the_constructed_bo3_spec() {
    let s = smoke(60);
    assert!(s.constructed && s.format == engine::az::Format::Bo3 && !s.pools);
    match &s.decks {
        DeckMode::ListsWithSideboards(p) => {
            assert_eq!((p[0].0.len(), p[0].1.len()), (60, 8));
            assert_eq!(p[0], p[1]);
        }
        other => panic!("{other:?}"),
    }
    // The evaluation spec of a decklist spec is the spec.
    let json = bo3_json(&names(&SMOKE_SIDEBOARD), 60);
    assert_eq!(MtgSpec::parse(&MtgSpec::eval_json(&json).unwrap()).unwrap(), s);
    // The full 75 parses: its five sideboard cards outside the cube are supported (ENGINE.md 122, 129.2).
    let full = delver();
    assert!(full.unsupported_sideboard(db()).is_empty(), "{:?}", full.unsupported_sideboard(db()));
    let side: Vec<String> = full.sideboard.iter().flat_map(|(n, s)| std::iter::repeat_n(s.clone(), *n as usize)).collect();
    match &MtgSpec::parse(&bo3_json(&side, 60)).unwrap().decks {
        DeckMode::ListsWithSideboards(p) => assert_eq!((p[0].0.len(), p[0].1.len(), p[1].1.len()), (60, 15, 15)),
        other => panic!("{other:?}"),
    }
    // A 75 holding cards the table does not know is refused, every one of them named (no silent drop, no
    // substitution).
    let mut bad = side.clone();
    bad.truncate(13);
    bad.extend(["Not A Card".to_string(), "Nor This One".to_string()]);
    let e = MtgSpec::parse(&bo3_json(&bad, 60)).unwrap_err();
    for c in ["Not A Card", "Nor This One"] {
        assert!(e.contains(c), "the refusal names {c}: {e}");
    }
    println!("two unknown cards: {e}");
    let main = full.main_names();
    let bad = |v: serde_json::Value| MtgSpec::parse(&v.to_string()).unwrap_err();
    let side8 = names(&SMOKE_SIDEBOARD);
    // The main is exactly 60 in best-of-three (every deck after sideboarding is): 61 refused (legal
    // in best-of-one, CR 100.2a).
    let more: Vec<String> = main.iter().cloned().chain(["Island".to_string()]).collect();
    assert!(bad(serde_json::json!({"constructed": true, "format": "bo3", "decks": {"p0": main, "p1": more, "p0_sideboard": side8, "p1_sideboard": side8}}))
        .contains("p1's constructed main has 61 cards"));
    MtgSpec::parse(&serde_json::json!({"constructed": true, "decks": {"p0": main, "p1": more}}).to_string()).unwrap();
    // CR 100.4a: a sideboard of at most 15; four copies over deck and sideboard.
    let big: Vec<String> = std::iter::repeat_n("Pyroblast".to_string(), 3).chain(std::iter::repeat_n("Spell Pierce".to_string(), 4)).chain(std::iter::repeat_n("Counterspell".to_string(), 4)).chain(std::iter::repeat_n("Fiery Confluence".to_string(), 4)).chain(["Karakas".to_string()]).collect();
    assert_eq!(big.len(), 16);
    assert!(bad(serde_json::json!({"constructed": true, "format": "bo3", "decks": {"p0": main, "p1": main, "p0_sideboard": big, "p1_sideboard": side8}})).contains("at most 15"));
    let five = vec!["Brainstorm".to_string()];
    assert!(bad(serde_json::json!({"constructed": true, "format": "bo3", "decks": {"p0": main, "p1": main, "p0_sideboard": five, "p1_sideboard": side8}})).contains("5 copies of Brainstorm"));
    // No pools, no hidden decklists (constructed hides its own way).
    assert!(bad(serde_json::json!({"constructed": true, "format": "bo3", "pools": true, "decks": {"p0": main, "p1": main}})).contains("pools"));
    assert!(bad(serde_json::json!({"constructed": true, "format": "bo3", "hidden_decklists": true, "decks": {"p0": main, "p1": main}})).contains("hidden_decklists"));
    // A best-of-three without sideboards is legal (CR 100.4a: up to 15): the deck can only be kept.
    let g = MtgAz::new_game(&MtgSpec::parse(&serde_json::json!({"constructed": true, "format": "bo3", "decks": {"p0": main, "p1": main}}).to_string()).unwrap(), 2, 3).unwrap();
    assert_eq!(g.m.pools[0].list.iter().map(|e| e.1 as usize).sum::<usize>(), 60);
}

#[test]
fn cr_100_4a_the_pool_is_the_75_with_no_free_basic_land() {
    let s = smoke(60);
    let g = MtgAz::new_game(&s, 2, 7).unwrap();
    let db = db();
    let island = db.expect_id("Island").unwrap();
    let main = Decklist::ids(db, &delver().main).unwrap();
    let side = Decklist::ids(db, &SMOKE_SIDEBOARD.iter().map(|&(n, s)| (n, s.to_string())).collect::<Vec<_>>()).unwrap();
    for p in 0..2 {
        let pool = &g.m.pools[p];
        // every card of the 68 with its copies, sorted, basic lands included as pool cards
        let mut all: Vec<u16> = main.iter().chain(&side).copied().collect();
        all.sort_unstable();
        let expanded: Vec<u16> = pool.list.iter().flat_map(|&(d, n)| std::iter::repeat_n(d, n as usize)).collect();
        assert_eq!(expanded, all);
        assert_eq!(pool.n_cards, pool.list.len(), "no free basic land entries");
        assert_eq!(pool.list.iter().find(|e| e.0 == island).map(|e| e.1), Some(1), "the one Island of the 75 is an entry of one copy");
        assert!(pool.first_picks.is_empty());
        assert_eq!((pool.min_deck, pool.max_deck), (60, 60));
        let mut d = g.m.deck(p as u8);
        let mut m = main.clone();
        d.sort_unstable();
        m.sort_unstable();
        assert_eq!(d, m, "game 1's deck is the registered main");
        assert_eq!(pool.sideboard(&g.m.decks[p]).len(), 8);
        assert!(g.m.deck_priors[p].as_ref().is_some_and(|x| x.name() == "constructed"));
    }
    assert!(g.m.constructed_bo3 && g.m.priors.is_none());
    // Game 1: open decklists (the best-of-one model): no hidden decks, the mains known.
    assert!(!g.g.spec().hidden_decks && !g.g.spec().deck_clouds && !g.g.spec().hidden_decklists);
}

/// Every decision sequence of sideboarding `pool` from `start` (DFS): the decks reached by `PASS`,
/// the decision nodes, the deepest sequence.
fn reachable(pool: &MatchPool, start: Vec<u8>) -> (Vec<Vec<u8>>, usize, usize) {
    fn go(pool: &MatchPool, sb: Sideboarding, depth: usize, out: &mut Vec<Vec<u8>>, nodes: &mut usize, deepest: &mut usize) {
        *nodes += 1;
        *deepest = (*deepest).max(depth);
        assert!(!sb.decision.options.is_empty(), "a dead end");
        for o in &sb.decision.options {
            let mut x = sb.clone();
            if x.choose(pool, o.action).unwrap() {
                let size: usize = x.counts.iter().map(|&c| c as usize).sum();
                assert!((pool.min_deck..=pool.max_deck).contains(&size), "PASS at {size}");
                out.push(x.counts.clone());
            } else {
                go(pool, x, depth + 1, out, nodes, deepest);
            }
        }
    }
    let (mut out, mut nodes, mut deepest) = (Vec::new(), 0, 0);
    go(pool, Sideboarding::new(0, start, pool), 1, &mut out, &mut nodes, &mut deepest);
    (out, nodes, deepest)
}

#[test]
fn cr_100_4a_constructed_sideboarding_reaches_every_legal_deck_exactly_once() {
    // A small constructed pool (MatchPool::constructed): a card of four copies, one of two, a basic
    // land of one copy (no free basic land entry: CR 100.4a, the 75 only), a card with copies in both
    // the main and the sideboard, a single card of the sideboard. Decks of exactly 6 (as a
    // constructed best-of-three's are exactly 60): the DFS over the decisions reaches every count
    // vector within the copies that sums to 6, each by one path, `PASS` only at 6.
    let (a, b, island, c, e) = (10u16, 11, 12, 13, 14);
    let main = vec![a, a, a, b, island, c];
    let side = vec![a, b, c, c, e];
    let (pool, start) = MatchPool::constructed(&main, &side);
    assert_eq!(pool.list, vec![(a, 4), (b, 2), (island, 1), (c, 3), (e, 1)]);
    assert_eq!((pool.n_cards, pool.min_deck, pool.max_deck), (5, 6, 6));
    assert_eq!(start, vec![3, 1, 1, 1, 0]);
    let mut want = BTreeSet::new();
    for x0 in 0..=4u8 {
        for x1 in 0..=2u8 {
            for x2 in 0..=1u8 {
                for x3 in 0..=3u8 {
                    for x4 in 0..=1u8 {
                        if x0 + x1 + x2 + x3 + x4 == 6 {
                            want.insert(vec![x0, x1, x2, x3, x4]);
                        }
                    }
                }
            }
        }
    }
    for st in [start.clone(), vec![4, 2, 0, 0, 0], vec![0, 1, 1, 3, 1], vec![2, 0, 1, 2, 1]] {
        let (got, nodes, deepest) = reachable(&pool, st.clone());
        let set: BTreeSet<Vec<u8>> = got.iter().cloned().collect();
        assert_eq!(set.len(), got.len(), "a deck reached twice from {st:?}");
        assert_eq!(set, want, "start {st:?}");
        // keeping the deck is one decision; changing every entry at most 1 + singles + 2 x the others
        assert!(deepest <= 1 + 2 + 2 * 3, "{deepest}");
        println!("constructed decks of exactly 6 from {st:?}: {} decks, {nodes} decision nodes, depth {deepest}", got.len());
    }
}

/// Sideboard `pool` from `start` to `target` by the canonical path (the entries that change, in list
/// order, then `PASS`): `Err` at the first option not offered.
fn sideboard_to(pool: &MatchPool, start: &[u8], target: &[u8]) -> Result<Vec<u8>, String> {
    let mut sb = Sideboarding::new(0, start.to_vec(), pool);
    let offered = |sb: &Sideboarding, a: u16| sb.decision.options.iter().any(|o| o.action == a);
    for k in 0..pool.list.len() {
        if target[k] == start[k] {
            continue;
        }
        let a = act::LIBRARY + k as u16;
        if !offered(&sb, a) {
            return Err(format!("entry {k} not offered"));
        }
        assert!(!sb.choose(pool, a).unwrap());
        if sb.decision.kind == DecisionKind::SideboardCount {
            let a = act::NUMBER + target[k] as u16;
            if !offered(&sb, a) {
                return Err(format!("entry {k}: count {} not offered", target[k]));
            }
            assert!(!sb.choose(pool, a).unwrap());
        }
    }
    if !offered(&sb, act::PASS) {
        return Err("PASS not offered".into());
    }
    assert!(sb.choose(pool, act::PASS).unwrap());
    Ok(sb.counts)
}

#[test]
fn a_constructed_deck_is_sideboarded_to_exactly_60_cards() {
    // The smoke's 68 (and a 75 of supported cards): every legal 60 drawn by the constructed prior
    // (every swap count) is reached by its canonical path; a deck of 59 or 61 by none; a random
    // walk finishes only at 60 (`PASS` offered exactly then); the heuristic keeps (`PASS` first).
    let db = db();
    let main = Decklist::ids(db, &delver().main).unwrap();
    let mut rng = Rng::new(60);
    let (mut paths, mut walks, mut walk_decisions) = (0, 0, 0);
    for side in [smoke_side(), side_15()] {
        let (pool, start) = MatchPool::constructed(&main, &side);
        let prior = ConstructedPrior::new(&main, &side);
        for _ in 0..200 {
            let s = prior.sample(&DeckWant { db, n: 60, shown: &[], companion: None, tilt: None, pick: None }, &mut rng);
            let (_, target) = MatchPool::constructed(&s.deck, &s.sideboard);
            assert_eq!(sideboard_to(&pool, &start, &target).unwrap(), target);
            paths += 1;
            // one card more or fewer: no path
            for (k, d) in [(0usize, 1i32), (pool.list.len() - 1, -1)] {
                let mut t = target.clone();
                let v = t[k] as i32 + d;
                if (0..=pool.list[k].1 as i32).contains(&v) {
                    t[k] = v as u8;
                    assert!(sideboard_to(&pool, &start, &t).is_err(), "a deck of {} reached", t.iter().map(|&x| x as u32).sum::<u32>());
                }
            }
        }
        for _ in 0..400 {
            let mut sb = Sideboarding::new(0, start.clone(), &pool);
            loop {
                let size: usize = sb.counts.iter().map(|&c| c as usize).sum();
                let pass = sb.decision.options.iter().any(|o| o.action == act::PASS);
                if sb.decision.kind == DecisionKind::Sideboard {
                    assert_eq!(pass, size == 60, "PASS offered at {size}");
                }
                let o = &sb.decision.options[rng.below(sb.decision.options.len() as u32) as usize];
                walk_decisions += 1;
                if sb.choose(&pool, o.action).unwrap() {
                    break;
                }
            }
            assert_eq!(sb.counts.iter().map(|&c| c as usize).sum::<usize>(), 60);
            assert!(sb.counts.iter().zip(&pool.list).all(|(&c, e)| c <= e.1), "more copies than the 75 has");
            walks += 1;
        }
    }
    println!("{paths} canonical paths, {walks} random walks ({:.1} decisions per walk)", walk_decisions as f64 / walks as f64);
}

fn smoke_side() -> Vec<u16> {
    Decklist::ids(db(), &SMOKE_SIDEBOARD.iter().map(|&(n, s)| (n, s.to_string())).collect::<Vec<_>>()).unwrap()
}

/// A 15-card sideboard of supported cards (the prior's tests at the full S = 15).
fn side_15() -> Vec<u16> {
    let list = [(3, "Pyroblast"), (3, "Force of Negation"), (1, "Karakas"), (1, "Unholy Heat"), (2, "Spell Pierce"), (3, "Counterspell"), (2, "Fiery Confluence")];
    Decklist::ids(db(), &list.iter().map(|&(n, s): &(u32, &str)| (n, s.to_string())).collect::<Vec<_>>()).unwrap()
}

#[test]
fn the_constructed_prior_meets_its_contract() {
    // The base prior (ENGINE.md 120.2): uniform over the number of swaps 0..=S, then uniform over the
    // decks with that number; conditioned exactly on the shown cards; a tilt of 1 is the untilted draw,
    // draw for draw; the expected fill is the draws' mean; the contract checked on every sample.
    let db = db();
    let main = Decklist::ids(db, &delver().main).unwrap();
    let side = side_15();
    let prior = ConstructedPrior::new(&main, &side);
    let by = prior.decks_by_swaps();
    assert_eq!(by.len(), 16);
    let total: f64 = by.iter().sum();
    let mean_uniform: f64 = by.iter().enumerate().map(|(s, &n)| s as f64 * n).sum::<f64>() / total;
    println!("legal 60s of this 75: {total:.4e}; by swaps {:?}", by.iter().map(|x| format!("{x:.3e}")).collect::<Vec<_>>());
    println!("uniform over the legal 60s: mean swaps {mean_uniform:.2} of 15; P(s <= 5) = {:.2e}", by[..=5].iter().sum::<f64>() / total);
    assert!(mean_uniform > 7.5, "uniform over 60s leans to many swaps: {mean_uniform}");
    let mut rng = Rng::new(113);
    let n = 3200;
    let mut hist = [0usize; 16];
    let plain = DeckWant { db, n: 60, shown: &[], companion: None, tilt: None, pick: None };
    for _ in 0..n {
        let s = prior.sample(&plain, &mut rng);
        check_sample(&prior, &plain, &s).unwrap();
        hist[prior.swaps_of(&s.deck)] += 1;
    }
    // uniform over s: each of the 16 counts near n / 16 = 200 (sd ~13.7); 5 sd
    for (s, &h) in hist.iter().enumerate() {
        assert!((h as f64 - 200.0).abs() < 70.0, "swaps {s}: {h} of {n} ({hist:?})");
    }
    println!("swaps of {n} untilted draws: {hist:?}");
    // Conditioned: shown cards of the sideboard and of the main, more copies than the main has.
    let id = |s: &str| db.expect_id(s).unwrap();
    let mut shown = vec![id("Pyroblast"), id("Pyroblast"), id("Unholy Heat"), id("Unholy Heat"), id("Unholy Heat"), id("Daze"), id("Karakas")];
    shown.sort_unstable();
    let want = DeckWant { db, n: 60, shown: &shown, companion: None, tilt: None, pick: None };
    let mut at_least = 0;
    for _ in 0..600 {
        let s = prior.sample(&want, &mut rng);
        check_sample(&prior, &want, &s).unwrap();
        assert!(holds(&s.deck, &shown));
        at_least += (prior.swaps_of(&s.deck) >= 4) as usize;
    }
    assert_eq!(at_least, 600, "two Pyroblasts, a third Unholy Heat and Karakas are four swaps at least");
    // A tilt of 1 is the untilted draw; a sharp tilt concentrates the deck on the card.
    let one = |_d: u16| 1.0;
    let tilted1 = DeckWant { db, n: 60, shown: &shown, companion: None, tilt: Some(&one), pick: None };
    let (mut r1, mut r2) = (Rng::new(5), Rng::new(5));
    for _ in 0..50 {
        assert_eq!(prior.sample(&want, &mut r1), prior.sample(&tilted1, &mut r2));
    }
    // (Four shown sideboard cards make many swaps likely - Bayes under the uniform swap count: a deck
    // of s swaps holds a given sideboard card about s / 15 of the time - so the untilted deck holds
    // about 2.5 of the 3 Counterspells; a tilt of 10^6 holds all three, one of 10^-6 none.)
    let cs = id("Counterspell");
    let (mut up, mut down, mut plain_n) = (0usize, 0usize, 0usize);
    for _ in 0..200 {
        for (w, n) in [(1e6, &mut up), (1e-6, &mut down)] {
            let f = move |d: u16| if d == cs { w } else { 1.0 };
            let tilted = DeckWant { db, n: 60, shown: &shown, companion: None, tilt: Some(&f), pick: None };
            let s = prior.sample(&tilted, &mut rng);
            check_sample(&prior, &tilted, &s).unwrap();
            assert!(holds(&s.deck, &shown));
            *n += s.deck.iter().filter(|&&d| d == cs).count();
        }
        plain_n += prior.sample(&want, &mut rng).deck.iter().filter(|&&d| d == cs).count();
    }
    assert!(up >= 595 && down <= 5 && (100..595).contains(&plain_n), "Counterspells in 200 decks: tilt 10^6 {up}, 10^-6 {down}, untilted {plain_n}");
    println!("Counterspells in 200 decks: tilt 10^6 {up}, 10^-6 {down}, untilted {plain_n}");
    // The expected fill is the draws' mean beyond the shown cards.
    let fill = prior.expected_fill(&want);
    let mut mean = vec![0.0f64; db.len()];
    let k = 4000;
    for _ in 0..k {
        let s = prior.sample(&want, &mut rng);
        let mut left = s.deck.clone();
        for x in &shown {
            left.remove(left.binary_search(x).unwrap());
        }
        for d in left {
            mean[d as usize] += 1.0 / k as f64;
        }
    }
    assert!((fill.iter().sum::<f64>() - (60 - shown.len()) as f64).abs() < 1e-6, "the fill is the deck's other cards");
    for (d, (&f, &m)) in fill.iter().zip(&mean).enumerate() {
        assert!((f - m).abs() < 0.08, "{}: expected fill {f:.3}, drawn {m:.3}", db.def(d as u16).name);
    }
    // The support is the 75's cards; the sideboard is the rest of the 75.
    let mut sup: Vec<u16> = main.iter().chain(&side).copied().collect();
    sup.sort_unstable();
    sup.dedup();
    assert_eq!(prior.support(), &sup[..]);
}

/// A match of the smoke spec played (mixed heuristic and random) to its first decision with
/// `game_no >= g` outside sideboarding (or between games when `between`), None if it ended first.
fn match_at(seed: u64, game: u8, between: bool) -> Option<MtgAz> {
    let sp = smoke(60);
    let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
    let mut h = Heuristic::new(seed);
    let mut rng = Rng::new(seed ^ 0xB03);
    let mut keys = Vec::new();
    while g.to_move().is_some() {
        if g.m.game_no >= game && (g.sideboarding().is_some() == between) && (between || g.g.turn().number >= 3) {
            return Some(g);
        }
        let a = mixed(&g, &mut h, &mut rng);
        g.step(a, &mut keys).unwrap();
    }
    None
}

#[test]
fn game_1_is_open_and_the_later_games_deal_hidden_60s() {
    // Game 1 (the best-of-one model): every world of either viewer holds the opponent's registered
    // main. Games 2-3: the opponent's 60 is hidden within their 75 (hidden decks, clouds kept, the
    // sideboard's size public): worlds deal legal 60s of the 75 holding every card the viewer has
    // seen this game - most of them not the real one - with the viewer's encoding and the decider's
    // actions unchanged, the viewer's own cards untouched.
    let (mut open, mut hidden, mut other) = (0, 0, 0);
    for seed in 0..12u64 {
        for game in [1u8, 2] {
            let Some(a) = match_at(seed, game, false) else { continue };
            let p = a.to_move().unwrap();
            if game >= 2 {
                assert!(a.g.spec().hidden_decks && a.g.spec().deck_clouds && !a.g.spec().hidden_decklists);
            }
            for viewer in [p, 1 - p] {
                let o = 1 - viewer as u8;
                let seen = shown(&a, viewer as u8);
                for w in 0..6u64 {
                    let mut d = a.clone();
                    d.determinize(viewer, &mut zero_search::rng::Rng::new(seed * 100 + w));
                    if viewer == p {
                        assert_eq!(legal(&d), legal(&a), "seed {seed}: the decider's actions");
                    } else {
                        assert_eq!(d.to_move(), Some(p));
                    }
                    assert!(enc(&d, viewer) == enc(&a, viewer), "seed {seed} game {}: the viewer's encoding", a.m.game_no);
                    assert_eq!(game_deck(&d, viewer as u8), game_deck(&a, viewer as u8), "the viewer's own cards");
                    assert_eq!(d.m.decks[viewer], a.m.decks[viewer]);
                    let world = game_deck(&d, o);
                    assert_eq!(world, d.m.deck(o), "the world's cards are the match's deck");
                    assert_eq!(world.len(), 60);
                    assert!(holds(&world, &seen), "the world's deck holds what the viewer saw");
                    let pool = &d.m.pools[o as usize];
                    assert_eq!(pool.list, a.m.pools[o as usize].list, "the 75 is public: the same list in every world");
                    assert!(d.m.decks[o as usize].iter().zip(&pool.list).all(|(&c, e)| c <= e.1));
                    assert_eq!(d.g.sideboard_size(o), 8, "the sideboard: the rest of the 68");
                    assert_eq!(d.g.player(o).sideboard.as_ref(), pool.sideboard(&d.m.decks[o as usize]).as_slice());
                    d.g.check_invariants().unwrap();
                    d.g.check_knowledge().unwrap();
                    if a.m.game_no == 1 {
                        assert_eq!(world, game_deck(&a, o), "game 1: the registered main");
                        open += 1;
                    } else {
                        hidden += 1;
                        other += (world != game_deck(&a, o)) as usize;
                    }
                }
            }
        }
    }
    println!("{open} game-1 worlds (the main), {hidden} later-game worlds, {other} with another 60 than the real one");
    assert!(open > 50 && hidden > 50 && other * 2 > hidden, "{open} {hidden} {other}");
}

#[test]
fn between_games_the_hidden_decks_are_dealt() {
    // Between games 1 and 2 the opponent's last deck is the registered main (kept in every world);
    // between games 2 and 3 it is hidden (dealt). The first sideboarder's chosen deck is hidden from
    // the second (dealt from the prior: two matches that differ only in that choice give the second
    // the same worlds), and the other player's sideboarding in progress restarts from the world's
    // last deck.
    let (mut after1, mut after2, mut blind) = (0, 0, 0);
    for seed in 0..40u64 {
        let Some(a) = match_at(seed, 1, true) else { continue };
        let sb = a.sideboarding().unwrap();
        let p = sb.decider() as usize;
        for viewer in [p, 1 - p] {
            let o = 1 - viewer;
            for w in 0..4u64 {
                let mut d = a.clone();
                d.determinize(viewer, &mut zero_search::rng::Rng::new(seed * 10 + w));
                assert!(enc(&d, viewer) == enc(&a, viewer), "seed {seed}: the viewer's encoding while sideboarding");
                if viewer == p {
                    assert_eq!(legal(&d), legal(&a), "the decider's options");
                    assert_eq!(d.sideboarding().unwrap().counts, sb.counts, "the decider's own sideboarding");
                } else {
                    // the decider's (the opponent's) sideboarding in progress: restarted from the world's last deck
                    let dsb = d.sideboarding().unwrap();
                    assert_eq!(dsb.counts, d.m.decks[o].to_vec());
                    assert_eq!(dsb.last, None);
                }
                assert_eq!(d.m.decks[viewer], a.m.decks[viewer], "the viewer's own last deck");
                if a.m.game_no == 1 {
                    assert_eq!(d.m.decks[o], a.m.decks[o], "after game 1: the registered main");
                    after1 += 1;
                } else {
                    after2 += 1;
                }
            }
        }
        // the second sideboarder blind to the first's choice: the first keeps (PASS) or changes its deck,
        // the second's worlds are the same
        if sb.decision.options.len() > 1 {
            let mut keep = a.clone();
            let mut change = a.clone();
            let mut keys = Vec::new();
            keep.step(act::PASS, &mut keys).unwrap();
            let mut rng = Rng::new(seed);
            while change.sideboarding().is_some_and(|s| s.decider() as usize == p) {
                let l: Vec<u16> = legal(&change).into_iter().filter(|&x| x != act::PASS || change.sideboarding().unwrap().last.is_some()).collect();
                let x = l[rng.below(l.len() as u32) as usize];
                change.step(x, &mut keys).unwrap();
            }
            if keep.sideboarding().is_some() && change.sideboarding().is_some() && keep.m.sb.as_ref().unwrap().done[p] != change.m.sb.as_ref().unwrap().done[p] {
                let q = 1 - p;
                for w in 0..4u64 {
                    let (mut x, mut y) = (keep.clone(), change.clone());
                    x.determinize(q, &mut zero_search::rng::Rng::new(w));
                    y.determinize(q, &mut zero_search::rng::Rng::new(w));
                    assert_eq!(x.m.sb.as_ref().unwrap().done[p], y.m.sb.as_ref().unwrap().done[p], "seed {seed}: the second sideboarder saw the first's choice");
                    assert!(enc(&x, q) == enc(&y, q) && enc(&keep, q) == enc(&change, q));
                    assert_eq!(legal(&x), legal(&y));
                }
                blind += 1;
            }
        }
    }
    for seed in 0..30u64 {
        let Some(a) = match_at(seed, 2, true) else { continue };
        let p = a.sideboarding().unwrap().decider() as usize;
        let mut other = 0;
        for w in 0..8u64 {
            let mut d = a.clone();
            d.determinize(p, &mut zero_search::rng::Rng::new(w));
            assert!(enc(&d, p) == enc(&a, p));
            other += (d.m.decks[1 - p] != a.m.decks[1 - p]) as usize;
            after2 += 1;
        }
        assert!(other > 0, "seed {seed}: after game 2 the opponent's last deck is dealt");
    }
    println!("{after1} worlds after game 1, {after2} after game 2, {blind} second sideboarders blind to the first's choice");
    assert!(after1 > 0 && after2 > 0 && blind > 0);
}

#[test]
fn the_heuristic_keeps_its_deck_and_constructed_matches_end() {
    // ENGINE.md 36.5: the heuristic's sideboarding is PASS (its 60 kept); matches end at two wins or
    // three games; every later game's decks are legal 60s of the 75.
    let sp = smoke(60);
    let (mut matches, mut games) = (0, 0);
    for seed in 0..6u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        let mut bots = [Heuristic::new(seed), Heuristic::new(seed + 1)];
        let mut keys = Vec::new();
        let start = [g.m.decks[0].clone(), g.m.decks[1].clone()];
        while let Some(p) = g.to_move() {
            let l = legal(&g);
            let a = match g.sideboarding() {
                Some(sb) => engine::az::bots::sideboard(sb, &l),
                None => bots[p].choose(&g.g, &l),
            };
            if g.sideboarding().is_some() {
                assert_eq!(a, act::PASS);
            }
            g.step(a, &mut keys).unwrap();
        }
        assert!(g.match_over());
        assert!(g.m.wins.contains(&2) || g.m.game_no == 3);
        assert_eq!([g.m.decks[0].clone(), g.m.decks[1].clone()], start, "the heuristic kept its 60");
        matches += 1;
        games += g.m.game_no as usize;
    }
    println!("{matches} heuristic matches, {games} games");
}

/// The constructed prior's cost (ENGINE.md 120.6; ignored: a measurement): a sample from the empty
/// request and from one with shown cards, and `determinize` in the later games of smoke matches
/// (the K = 32 cached decks of each request drawn at its first deal, then one of them picked).
#[test]
#[ignore]
fn constructed_deal_cost() {
    let db = db();
    let main = Decklist::ids(db, &delver().main).unwrap();
    let prior = ConstructedPrior::new(&main, &side_15());
    let mut rng = Rng::new(1);
    let shown: Vec<u16> = {
        let mut v = vec![db.expect_id("Pyroblast").unwrap(), db.expect_id("Daze").unwrap(), db.expect_id("Volcanic Island").unwrap()];
        v.sort_unstable();
        v
    };
    for (what, sh) in [("no shown card", &[][..]), ("three shown cards", &shown[..])] {
        let want = DeckWant { db, n: 60, shown: sh, companion: None, tilt: None, pick: None };
        let t = std::time::Instant::now();
        let n = 4000;
        for _ in 0..n {
            std::hint::black_box(prior.sample(&want, &mut rng));
        }
        println!("a constructed prior sample, {what}: {:.1} us", t.elapsed().as_secs_f64() * 1e6 / n as f64);
    }
    let (mut n, mut first, mut total, mut positions) = (0u32, 0.0f64, 0.0f64, 0u32);
    for seed in 0..12u64 {
        let Some(a) = match_at(seed, 2, false) else { continue };
        positions += 1;
        let p = a.to_move().unwrap();
        for k in 0..64u64 {
            let mut d = a.clone();
            let t = std::time::Instant::now();
            d.determinize(p, &mut zero_search::rng::Rng::new(k));
            let s = t.elapsed().as_secs_f64() * 1e6;
            if k == 0 {
                first += s;
            } else {
                total += s;
                n += 1;
            }
        }
    }
    println!("determinize in a later game: the request's first deal {:.0} us (the K decks drawn), then {:.1} us a deal ({n} deals)", first / positions as f64, total / n as f64);
}

/// A second constructed 75 of supported cards, different from the Delver list (a deck-agnostic test:
/// constructed best-of-three takes any two 75s, ENGINE.md 120): a 60 with basic lands past four copies
/// (8 Island, 6 Mountain: entries of 8 and 7 copies, no free basic land) and a 12-card sideboard.
const OTHER_MAIN: [(u32, &str); 15] = [
    (4, "Lightning Bolt"),
    (4, "Brainstorm"),
    (4, "Ponder"),
    (4, "Counterspell"),
    (4, "Spell Pierce"),
    (2, "Fiery Confluence"),
    (4, "Force of Will"),
    (4, "Dragon's Rage Channeler"),
    (2, "Murktide Regent"),
    (4, "Volcanic Island"),
    (4, "Scalding Tarn"),
    (8, "Island"),
    (6, "Mountain"),
    (4, "Mishra's Bauble"),
    (2, "Unholy Heat"),
];
const OTHER_SIDE: [(u32, &str); 6] = [(3, "Pyroblast"), (3, "Force of Negation"), (1, "Karakas"), (2, "Unholy Heat"), (2, "Fiery Confluence"), (1, "Mountain")];

#[test]
fn two_different_75s_play_constructed_matches() {
    // p0 the Delver main and its supported sideboard (68), p1 another 75 (60 + 12): each pool is its
    // own 75 (the list, the copies, the sideboard's size), each player's worlds deal the OTHER player's
    // 60 from that player's 75 (its own prior), sideboarding keeps 60 for both, matches end.
    let db = db();
    let main0 = delver().main_names();
    let (main1, side1) = (names(&OTHER_MAIN), names(&OTHER_SIDE));
    assert_eq!((main1.len(), side1.len()), (60, 12));
    let json = serde_json::json!({"max_turns": 60, "constructed": true, "format": "bo3",
        "decks": {"p0": main0, "p1": main1, "p0_sideboard": names(&SMOKE_SIDEBOARD), "p1_sideboard": side1}})
    .to_string();
    let sp = MtgSpec::parse(&json).unwrap();
    let island = db.expect_id("Island").unwrap();
    let mountain = db.expect_id("Mountain").unwrap();
    let (mut worlds, mut later, mut games) = (0, 0, 0);
    for seed in 0..10u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        assert_ne!(g.m.pools[0].list, g.m.pools[1].list);
        assert_eq!(g.m.pools[1].list.iter().find(|e| e.0 == island).map(|e| e.1), Some(8));
        assert_eq!(g.m.pools[1].list.iter().find(|e| e.0 == mountain).map(|e| e.1), Some(7));
        assert_eq!(g.m.pools[0].list.iter().find(|e| e.0 == mountain), None, "no free basic land entry");
        assert_eq!((g.g.sideboard_size(0), g.g.sideboard_size(1)), (8, 12));
        let mut h = Heuristic::new(seed);
        let mut rng = Rng::new(seed ^ 0x75);
        let mut keys = Vec::new();
        let mut step = 0;
        while let Some(p) = g.to_move() {
            if step % 37 == 0 {
                for viewer in [p, 1 - p] {
                    let o = 1 - viewer;
                    let mut d = g.clone();
                    d.determinize(viewer, &mut zero_search::rng::Rng::new(seed * 1000 + step));
                    assert!(enc(&d, viewer) == enc(&g, viewer), "seed {seed}: the viewer's encoding");
                    if viewer == p {
                        assert_eq!(legal(&d), legal(&g));
                    }
                    assert_eq!(d.m.pools[o].list, g.m.pools[o].list, "the opponent's own 75");
                    assert_eq!(d.m.pools[viewer], g.m.pools[viewer]);
                    let counts = &d.m.decks[o];
                    assert_eq!(counts.iter().map(|&c| c as usize).sum::<usize>(), 60);
                    assert!(counts.iter().zip(&d.m.pools[o].list).all(|(&c, e)| c <= e.1));
                    if d.sideboarding().is_none() {
                        assert_eq!(game_deck(&d, o as u8), d.m.deck(o as u8), "the world's cards are the dealt 60 of their 75");
                        assert!(holds(&game_deck(&d, o as u8), &shown(&g, viewer as u8)));
                        d.g.check_invariants().unwrap();
                        d.g.check_knowledge().unwrap();
                    }
                    later += (g.m.game_no >= 2 && g.sideboarding().is_none()) as usize;
                    worlds += 1;
                }
            }
            let a = mixed(&g, &mut h, &mut rng);
            let before = g.m.game_no;
            g.step(a, &mut keys).unwrap();
            if g.m.game_no != before {
                for q in 0..2usize {
                    assert_eq!(g.m.decks[q].iter().map(|&c| c as usize).sum::<usize>(), 60);
                    assert_eq!(g.g.sideboard_size(q as u8), [8, 12][q]);
                }
            }
            step += 1;
        }
        assert!(g.match_over());
        games += g.m.game_no as usize;
    }
    println!("two 75s: {games} games in 10 matches, {worlds} worlds ({later} in later games)");
    assert!(later > 20, "{later}");
}

// ---- Show and Tell's face-down pick and the hidden 60 (ENGINE.md 129.3) ----

/// A Sneak and Show 75 for a constructed best-of-three on this layout: MTGGoldfish 8014220
/// (`data/decks/sneak_show_8014220.json`) with its Disruptor Flute (refused on layout v6: its card name
/// needs layout v7) swapped for a second Pyroblast. (main, sideboard) names.
fn sneak_show_75() -> (Vec<String>, Vec<String>) {
    let d = Decklist::parse(include_str!("../../data/decks/sneak_show_8014220.json")).unwrap();
    let side: Vec<String> = d
        .sideboard
        .iter()
        .flat_map(|(n, s)| std::iter::repeat_n(if s == "Disruptor Flute" { "Pyroblast".to_string() } else { s.clone() }, *n as usize))
        .collect();
    assert!(!side.iter().any(|s| s == "Disruptor Flute") && side.len() == 15);
    (d.main_names(), side)
}

/// "An artifact, creature, enchantment, or land card" (Show and Tell's kind), read from the card's
/// definition: the test's own reading of the Oracle text, beside the engine's filter.
fn snt_kind(d: u16) -> bool {
    use engine::db::types;
    db().def(d).types & (types::ARTIFACT | types::CREATURE | types::ENCHANTMENT | types::LAND) != 0
}

fn ids_of(names: &[String]) -> Vec<u16> {
    let mut v: Vec<u16> = names.iter().map(|n| db().id(n).unwrap_or_else(|| panic!("{n}"))).collect();
    v.sort_unstable();
    v
}

#[test]
fn cr_101_4a_a_face_down_pick_reaches_the_deck_request_exactly() {
    // ruling: a107ed89
    // The request of a hidden 60 (ENGINE.md 129.3). The viewer has seen 37 of the Sneak and Show main's
    // 38 artifact, creature, enchantment and land cards (all but one Emrakul); the sideboard holds 7 cards
    // of other kinds. Untold, the constructed prior deals 60s whose unseen cards hold none of the kind
    // (that Emrakul sided out for instants and sorceries): in such a world the opponent's face-down
    // Show and Tell pick, which the viewer knows was made, can be no card at all - `determinize` could
    // never deal one. The request carrying the pick (its chance a + b y given the deck, y the deck's cards
    // of the kind beyond those seen; here a = 0: no hand card of the kind is known) never deals one, and
    // deals the exact posterior: the plain prior's decks reweighed by a + b y.
    let (main, side) = sneak_show_75();
    let (main, side) = (ids_of(&main), ids_of(&side));
    let prior = ConstructedPrior::new(&main, &side);
    let emrakul = db().id("Emrakul, the Aeons Torn").unwrap();
    let mut shown: Vec<u16> = main.iter().copied().filter(|&d| snt_kind(d)).collect();
    shown.remove(shown.iter().position(|&d| d == emrakul).unwrap());
    assert_eq!(shown.len(), 37);
    let legal: Vec<u16> = {
        let mut v: Vec<u16> = prior.support().iter().copied().filter(|&d| snt_kind(d)).collect();
        v.dedup();
        v
    };
    let beyond = |deck: &[u16]| deck.iter().filter(|&&d| snt_kind(d)).count() - shown.len();
    let plain = DeckWant { db: db(), n: 60, shown: &shown, companion: None, tilt: None, pick: None };
    // (a = 0, b = 1: the chance is proportional to y)
    let pw = engine::az::hidden_decks::PickWant { legal: legal.clone(), a: 0.0, b: 1.0 };
    let picked = DeckWant { db: db(), n: 60, shown: &shown, companion: None, tilt: None, pick: Some(&pw) };
    const N: usize = 20_000;
    let mut rng = Rng::new(129);
    let (mut none, mut wsum, mut wy) = (0usize, 0.0f64, 0.0f64);
    let mut by_y_plain = [0.0f64; 16];
    for _ in 0..N {
        let s = prior.sample(&plain, &mut rng);
        check_sample(&prior, &plain, &s).unwrap();
        let mut deck = s.deck.clone();
        deck.sort_unstable();
        let y = beyond(&deck);
        none += (y == 0) as usize;
        // the posterior by importance: each plain deck weighed by its chance y
        wsum += y as f64;
        wy += (y * y) as f64;
        by_y_plain[y] += y as f64;
    }
    let mut by_y_picked = [0.0f64; 16];
    let mut sy = 0.0f64;
    for _ in 0..N {
        let s = prior.sample(&picked, &mut rng);
        check_sample(&prior, &picked, &s).unwrap();
        let mut deck = s.deck.clone();
        deck.sort_unstable();
        let y = beyond(&deck);
        assert!(y > 0, "a deck with no card of the kind beyond those seen: no legal face-down pick");
        by_y_picked[y] += 1.0;
        sy += y as f64;
    }
    let (m_plain_w, m_picked) = (wy / wsum, sy / N as f64);
    println!("plain prior: {none} of {N} decks with no unseen card of the kind; posterior mean y by importance {m_plain_w:.4}, by the request {m_picked:.4}");
    assert!(none > 20, "the plain prior deals 60s without a legal pick here: {none}");
    // The distribution of y: the request's against the importance-weighed plain prior's.
    for y in 1..16 {
        let (p, q) = (by_y_plain[y] / wsum, by_y_picked[y] / N as f64);
        let se = (p * (1.0 - p) / N as f64).sqrt() * 2.0 + 1e-4;
        assert!((p - q).abs() < 4.0 * se, "y = {y}: importance {p:.4}, request {q:.4}");
    }
    // `expected_fill` under the pick: the request's own means.
    let fill = prior.expected_fill(&picked);
    let e_em = fill[emrakul as usize];
    let mut m_em = 0.0;
    let mut rng = Rng::new(130);
    for _ in 0..N {
        let s = prior.sample(&picked, &mut rng);
        m_em += s.deck.iter().filter(|&&d| d == emrakul).count() as f64 / N as f64;
    }
    // (the fill is beyond the shown copies: two of the three Emrakuls are shown)
    let m_em = m_em - 2.0;
    println!("Emrakul's expected fill under the pick {e_em:.4}, sampled {m_em:.4}");
    assert!((e_em - m_em).abs() < 0.03, "{e_em} {m_em}");
    // A request without a pick draws as before (ENGINE.md 120.2): the same decks, draw for draw.
    let (mut r1, mut r2) = (Rng::new(5), Rng::new(5));
    let again = DeckWant { db: db(), n: 60, shown: &shown, companion: None, tilt: None, pick: None };
    assert_eq!(prior.sample(&plain, &mut r1), prior.sample(&again, &mut r2));
}

/// The steering of `snt_match`: Sneak and Show (P0) plays a land, casts Show and Tell and Lotus Petal when
/// it can and puts a card with Show and Tell; the Delver player (P1) only passes priority; every other
/// decision at random (mulligans, payments, the play-or-draw choice, discards).
fn snt_policy(a: &MtgAz, rng: &mut Rng) -> u16 {
    let l = legal(a);
    if a.sideboarding().is_some() {
        return act::PASS;
    }
    let d = a.pending_decision().unwrap();
    let find = |f: &dyn Fn(engine::Choice) -> bool| d.options.iter().find(|o| f(o.choice)).map(|o| o.action);
    if d.kind == DecisionKind::Priority {
        if d.player == 0 {
            let named = |c: engine::Choice, n: &str| matches!(c, engine::Choice::Cast(x) if a.g.name(x) == n);
            if let Some(x) = find(&|c| named(c, "Show and Tell"))
                .or_else(|| find(&|c| matches!(c, engine::Choice::Play(_) | engine::Choice::PlayFace(_))))
                .or_else(|| find(&|c| named(c, "Lotus Petal")))
            {
                return x;
            }
        }
        return act::PASS;
    }
    if d.kind == DecisionKind::ChooseHandCard && d.player == 0 {
        if let Some(x) = find(&|c| c != engine::Choice::Pass) {
            return x;
        }
    }
    l[rng.below(l.len() as u32) as usize]
}

/// A constructed best-of-three, Sneak and Show (`sneak_show_75`, P0) against the Delver 75 (P1), one
/// episode per game: P0 concedes game 1 at once (CR 104.3a), both keep their mains, and in game 2 - P0's
/// 60 hidden from P1 - `snt_policy` plays until P0 puts a card with Show and Tell. -> (the state where P0
/// chooses its card, the state after it chose: P1 to choose), or None (no such pick in this match).
fn snt_match(seed: u64) -> Option<(MtgAz, MtgAz)> {
    let (sm, ss) = sneak_show_75();
    let d = delver();
    let dside: Vec<String> = d.sideboard.iter().flat_map(|(n, s)| std::iter::repeat_n(s.clone(), *n as usize)).collect();
    let spec = MtgSpec::parse(
        &serde_json::json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true,
            "decks": {"p0": sm, "p1": d.main_names(), "p0_sideboard": ss, "p1_sideboard": dside}})
        .to_string(),
    )
    .unwrap();
    let g = MtgAz::new_game(&spec, 2, seed).unwrap();
    let mut a = match g.next_episode(Some(0)).unwrap() {
        zero_search::Next::Continue(a) => a,
        zero_search::Next::Over(_) => panic!("one game conceded is no match"),
    };
    let mut rng = Rng::new(seed ^ 0x5A7);
    let mut keys = Vec::new();
    for _ in 0..20_000 {
        if a.to_move().is_none() {
            return None;
        }
        let d = a.pending_decision().unwrap();
        let resolving_snt = a.g.stack().last().is_some_and(|it| matches!(it.kind, engine::game::StackKind::Spell { card } if a.g.name(card) == "Show and Tell"));
        if a.m.game_no == 2 && d.kind == DecisionKind::ChooseHandCard && d.player == 0 && resolving_snt && d.options.len() > 1 {
            let before = a.clone();
            let x = snt_policy(&a, &mut rng);
            a.step(x, &mut keys).unwrap();
            let next = a.pending_decision()?;
            if next.player != 1 || next.kind != DecisionKind::ChooseHandCard {
                return None; // (P1's hand is empty: it is asked nothing)
            }
            return Some((before, a));
        }
        let x = snt_policy(&a, &mut rng);
        a.step(x, &mut keys).unwrap();
    }
    None
}

/// P1 declines in world `w` (P1 to choose its Show and Tell card): the card P0's face-down choice put
/// onto the battlefield.
fn snt_put(w: &MtgAz) -> u16 {
    let mut w = w.clone();
    let before: Vec<engine::CardIdx> = w.g.battlefield().iter().copied().filter(|&c| w.g.obj(c).owner == 0).collect();
    w.step(act::PASS, &mut Vec::new()).unwrap();
    let new: Vec<engine::CardIdx> = w.g.battlefield().iter().copied().filter(|&c| w.g.obj(c).owner == 0 && !before.contains(&c)).collect();
    assert_eq!(new.len(), 1, "P0's face-down choice entered: {new:?}");
    w.g.ident(new[0])
}

#[test]
fn cr_101_4a_show_and_tell_in_a_later_game_deals_a_hidden_60_with_a_legal_pick() {
    // ruling: a107ed89
    // ENGINE.md 129.3: in game 2 of a constructed best-of-three P0's 60 is hidden from P1. P0 resolves Show
    // and Tell and chooses a card face down; P1, to choose, knows only that one was chosen. Every world of
    // P1 - the K-sample deal (32), the belief- and deck-tilted deal - deals P0 a legal 60 of its 75 holding
    // what P1 saw, the same decision for P1, and a face-down choice of the kind. With fresh samples (K = 0)
    // the worlds' decks are the exact posterior: the same as P1's plain worlds just before the choice (the
    // same knowledge) kept only when a card of P0's hand drawn uniformly is of the kind.
    let (seed, (s0, s1)) = (0..40u64).find_map(|s| snt_match(s).map(|x| (s, x))).expect("a Show and Tell pick in game 2 within 40 matches");
    assert!(s1.g.spec().hidden_decks && s1.m.game_no == 2);
    let seen = shown(&s1, 1);
    println!(
        "match {seed}, game 2 turn {}: P0's hand {} cards, P1 has seen {} of P0's cards ({} of the kind)",
        s1.g.turn().number,
        s1.g.player(0).hand.len(),
        seen.len(),
        seen.iter().filter(|&&d| snt_kind(d)).count()
    );
    let real = legal(&s1);
    let mut zr = zero_search::rng::Rng::new(129);
    let (mut worlds, mut other) = (0usize, 0usize);
    let check = |w: &MtgAz| {
        assert_eq!(legal(w), real, "P1's decision in its world");
        let deck = game_deck(w, 0);
        assert_eq!(deck.len(), 60);
        assert!(holds(&deck, &seen), "the world's 60 holds what P1 saw");
        assert!(w.m.decks[0].iter().zip(&w.m.pools[0].list).all(|(&c, e)| c <= e.1), "a 60 of the 75");
        w.g.check_invariants().unwrap();
        w.g.check_knowledge().unwrap();
        let put = snt_put(w);
        assert!(snt_kind(put), "P0's face-down choice is {}: not an artifact, creature, enchantment or land", db().def(put).name);
        deck
    };
    // the K-sample deal and the tilted deals
    for i in 0..200u32 {
        let mut w = s1.clone();
        if i % 3 == 0 {
            w.determinize(1, &mut zr);
        } else {
            let d = engine::az::identity::D;
            let raw: Vec<f32> = (0..2 * d).map(|k| ((k * 7919 + i as usize) % 97 + 1) as f32).collect();
            let (b, dk) = (raw[..d].iter().sum::<f32>(), raw[d..].iter().sum::<f32>());
            let pred: Vec<f32> = raw.iter().enumerate().map(|(k, &x)| x / if k < d { b } else { dk }).collect();
            let hand = if i % 3 == 1 { 1.0 } else { 0.0 };
            let bel = zero_search::game::Belief { pred, tilt: hand, clip: 20.0, tilts: vec![0.0, 1.0] };
            w.determinize_with(1, &mut zr, &bel);
        }
        other += (check(&w) != game_deck(&s1, 0)) as usize;
        worlds += 1;
    }
    println!("{worlds} worlds of P1 at the face-down choice, {other} with another 60 than the real one");
    assert!(other * 2 > worlds);
    // Exactness with fresh samples (K = 0): P1's worlds after the choice against its plain worlds before it,
    // each kept when a uniform card of P0's hand is of the kind. The statistic: the 60's cards of the kind.
    let kinds = |deck: &[u16]| deck.iter().filter(|&&d| snt_kind(d)).count() as f64;
    let (mut before, mut after) = (s0.clone(), s1.clone());
    before.m.deal.k = 0;
    after.m.deal.k = 0;
    let mut tr = Rng::new(1293);
    let (mut acc, mut plain) = (Vec::new(), Vec::new());
    for _ in 0..20_000 {
        let mut w = before.clone();
        w.determinize(1, &mut zr);
        let deck = game_deck(&w, 0);
        plain.push(kinds(&deck));
        let hand = &w.g.player(0).hand;
        if snt_kind(w.g.ident(hand[tr.below(hand.len() as u32) as usize])) {
            acc.push(kinds(&deck));
        }
    }
    let fresh: Vec<f64> = (0..20_000)
        .map(|_| {
            let mut w = after.clone();
            w.determinize(1, &mut zr);
            kinds(&check(&w))
        })
        .collect();
    let stat = |v: &[f64]| {
        let n = v.len() as f64;
        let m = v.iter().sum::<f64>() / n;
        (m, (v.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0) / n).sqrt())
    };
    let ((ma, sa), (mf, sf), (mp, sp)) = (stat(&acc), stat(&fresh), stat(&plain));
    println!(
        "P0's 60's cards of the kind: the posterior by rejection {ma:.3} +- {sa:.3} ({} of {} plain worlds kept), the request {mf:.3} +- {sf:.3}, the plain prior {mp:.3}",
        acc.len(),
        plain.len()
    );
    assert!((ma - mf).abs() < 4.0 * (sa * sa + sf * sf).sqrt(), "the request's worlds are not the posterior: {ma} {mf}");
    // ... and the plain prior's are not (the deal before ENGINE.md 129.3: the 60 untold of the pick)
    assert!((ma - mp).abs() > 3.0 * (sa * sa + sp * sp).sqrt(), "the pick does not move the posterior here: {ma} {mp}");
}
