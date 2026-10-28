//! The zero-core game `team_draft` (DESIGN.md 2.3, ENGINE.md 41): the packs,
//! the seats and passes, picks, pools, learned deckbuilding, the pairings, the
//! first-picks rule, the information model (`determinize` keeps what the
//! viewer knows; equal keys mean equal features), clones, `force_chance`, a
//! random-actions fuzz of whole events, an event resolved by heuristic-played
//! matches, the bots, the spec and the layout file. Deckbuilding as picks
//! (ENGINE.md 47): any pool card not in the deck or `DONE`; every legal deck
//! is one end state, reached by each order of its pool cards. `DRAFT_SCALE=k` plays k times as many events
//! in the sampled-state tests; `DRAFT_FUZZ=n` sets the fuzz's events (default
//! 200).

use engine::az::draft::{
    self, cube, DraftSpec, Phase, TeamDraftAz, COUNT, DECK_MIN, DECK_SIZE, DONE, KEY_SEE, KEY_TEAM, MAX_BASIC,
};
use engine::az::draft_bots::{self, Consensus};
use engine::az::draft_encode::{self, NUM_FEATURES};
use engine::az::pools;
use engine::az::decks::DeckGen;
use engine::hidden::{key, key_kind, key_payload, key_player, HIDDEN};
use engine::rng::Rng;
use zero_search::game::{make_bot, Bot, SearchGame};

fn scale(n: u64) -> u64 {
    n * std::env::var("DRAFT_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

fn spec(json: &str) -> DraftSpec {
    DraftSpec::parse(json).unwrap()
}

fn new(players: usize, seed: u64) -> TeamDraftAz {
    TeamDraftAz::new(&DraftSpec::default(), players, seed).unwrap()
}

fn legal(g: &TeamDraftAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn enc(g: &TeamDraftAz, viewer: u8) -> Vec<f32> {
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode(viewer as usize, &mut x);
    x
}

fn zr(seed: u64) -> zero_search::rng::Rng {
    zero_search::rng::Rng::new(seed)
}

/// `step(a)` then every single-option decision (zero-core's transition).
fn transition(g: &mut TeamDraftAz, a: u16, keys: &mut Vec<u64>) {
    keys.clear();
    g.step(a, keys).unwrap();
    while g.to_move().is_some() {
        let l = legal(g);
        if l.len() != 1 {
            break;
        }
        g.step(l[0], keys).unwrap();
    }
}

/// Random actions to the deferred terminal.
fn finish_random(g: &mut TeamDraftAz, rng: &mut Rng) -> u64 {
    let mut keys = Vec::new();
    let mut n = 0;
    while g.to_move().is_some() {
        let l = legal(g);
        assert!(!l.is_empty());
        let a = l[rng.below(l.len() as u32) as usize];
        g.step(a, &mut keys).unwrap();
        n += 1;
        assert!(n < 100_000);
    }
    n
}

/// States of random events (players seats), one every ~`every` decisions.
fn states(players: usize, events: u64, every: u32, seed: u64) -> Vec<TeamDraftAz> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();
    let mut keys = Vec::new();
    for e in 0..events {
        let mut g = new(players, seed * 1000 + e);
        while g.to_move().is_some() {
            if rng.below(every) == 0 {
                out.push(g.clone());
            }
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
    }
    out
}

#[test]
fn packs_are_seeded_disjoint_and_full() {
    let cube = cube().unwrap();
    assert_eq!(cube.cards.len(), 541, "the cube less its five basic lands");
    for players in [2usize, 4, 6, 8] {
        let g = new(players, 7);
        g.check_invariants().unwrap();
        let ev = &g.ev;
        assert_eq!(ev.n_packs(), 3 * players);
        let mut all: Vec<u16> = g.rest().to_vec();
        for p in 0..ev.n_packs() {
            let pk = g.pack(p);
            assert_eq!(pk.len(), 15);
            assert!(pk.windows(2).all(|w| w[0] < w[1]));
            all.extend_from_slice(pk);
        }
        assert_eq!(g.rest().len(), 541 - 45 * players);
        all.sort_unstable();
        all.dedup();
        assert_eq!(all, cube.cards, "every card once: in a pack or undrafted");
        let h = new(players, 7);
        assert_eq!((0..ev.n_packs()).map(|p| h.pack(p).to_vec()).collect::<Vec<_>>(), (0..ev.n_packs()).map(|p| g.pack(p).to_vec()).collect::<Vec<_>>());
        let k = new(players, 8);
        assert_ne!(k.pack(0), g.pack(0));
    }
    // Settings for small tests.
    let g = TeamDraftAz::new(&spec(r#"{"packs": 2, "pack_size": 5}"#), 8, 1).unwrap();
    assert_eq!(g.ev.pool_size(), 10);
    assert_eq!(g.pack(0).len(), 5);
}

#[test]
fn seats_alternate_and_every_pass_goes_to_an_opponent() {
    for players in [2usize, 4, 8] {
        let g = new(players, 1);
        let ev = g.ev.clone();
        let n = ev.n as usize;
        for p in 0..ev.n_packs() {
            let (r, opener) = (p / n, p % n);
            assert_eq!(ev.seat_at(p, 0) as usize, opener);
            for t in 0..15 {
                let s = ev.seat_at(p, t);
                assert_eq!(ev.pack_at(s, r, t), p);
                if t + 1 < 15 && n > 1 {
                    let s2 = ev.seat_at(p, t + 1);
                    assert_ne!(s % 2, s2 % 2, "a pass to a teammate");
                    let want = if r == 1 { (s as usize + n - 1) % n } else { (s as usize + 1) % n };
                    assert_eq!(s2 as usize, want, "round {r} passes {}", if r == 1 { "right" } else { "left" });
                }
            }
        }
        // Seats 0, 2, ... are team A.
        assert_eq!(ev.team_mask(0).count_ones() as usize, n / 2);
    }
    // In a game: after a pick round each seat holds what its neighbour passed.
    let mut g = new(8, 3);
    let before: Vec<Vec<u16>> = (0..8).map(|s| g.pack(g.pack_held(s).unwrap()).to_vec()).collect();
    let mut picked = vec![0u16; 8];
    let mut keys = Vec::new();
    for s in 0..8 {
        assert_eq!(g.to_move(), Some(s));
        let l = legal(&g);
        picked[s] = l[3];
        g.step(l[3], &mut keys).unwrap();
    }
    for s in 0..8u8 {
        let from = ((s as usize) + 7) % 8;
        let mut want = before[from].clone();
        want.retain(|&c| c != picked[from]);
        assert_eq!(g.pack(g.pack_held(s).unwrap()), &want[..], "seat {s} receives seat {from}'s pack");
    }
    // Every seat saw its new pack: one key per card (no observer).
    assert_eq!(keys.len(), 8 * 14);
    assert!(keys.iter().all(|&k| key_kind(k) == KEY_SEE));
}

#[test]
fn picks_are_the_pack_and_illegal_actions_are_errors() {
    let mut g = new(4, 2);
    let l = legal(&g);
    assert_eq!(l, g.pack(g.pack_held(0).unwrap()));
    let mut keys = Vec::new();
    let not_in = cube().unwrap().cards.iter().copied().find(|c| !l.contains(c)).unwrap();
    assert!(g.clone().step(not_in, &mut keys).is_err());
    assert!(g.clone().step(DONE, &mut keys).is_err());
    assert!(g.clone().step(COUNT, &mut keys).is_err());
    g.step(l[0], &mut keys).unwrap();
    assert!(keys.is_empty(), "a pick reveals nothing until the packs pass");
    assert_eq!(g.picks(0), vec![(l[0], 0, 0)]);
    assert_eq!(g.to_move(), Some(1));
    // The deferred terminal refuses actions.
    let mut rng = Rng::new(1);
    finish_random(&mut g, &mut rng);
    assert_eq!(g.phase(), Phase::Done);
    assert!(g.step(DONE, &mut keys).is_err());
}

#[test]
fn every_pool_has_45_cards_and_decks_are_legal() {
    let cube = cube().unwrap();
    let mut rng = Rng::new(4);
    for (e, players) in [8usize, 4, 2, 8, 4, 6].into_iter().enumerate() {
        let mut g = new(players, 40 + e as u64);
        finish_random(&mut g, &mut rng);
        g.check_invariants().unwrap();
        let n = players as u8;
        let mut all = Vec::new();
        for s in 0..n {
            let pool = g.pool(s);
            assert_eq!(pool.len(), 45, "seat {s}");
            assert_eq!(g.first_picks(s).len(), 3);
            all.extend_from_slice(&pool);
            let (deck, side) = g.deck_and_sideboard(s).unwrap();
            assert_eq!(deck.len(), DECK_SIZE, "a deck is exactly 40 cards (ENGINE.md 42)");
            assert!(g.build(s).unwrap().pool_cards() >= DECK_MIN as usize, "at least the minimum of pool cards (ENGINE.md 47)");
            let mut non_basic: Vec<u16> = deck.iter().map(|&d| cube.ident_of(d)).filter(|c| !cube.basic_ids.contains(c)).collect();
            for &b in &cube.basic_defs {
                assert!(deck.iter().filter(|&&d| d == b).count() <= MAX_BASIC as usize);
            }
            non_basic.extend(side.iter().map(|&d| cube.ident_of(d)));
            non_basic.sort_unstable();
            assert_eq!(non_basic, pool, "deck + sideboard = the pool (and basics)");
        }
        all.sort_unstable();
        let n_all = all.len();
        all.dedup();
        assert_eq!(all.len(), n_all);
        assert_eq!(n_all, 45 * players);
    }
    // The consensus bot builds the heuristic deckbuilder's deck.
    let mut g = new(4, 9);
    let mut bots: Vec<_> = (0..4).map(|s| make_bot::<TeamDraftAz>(&DraftSpec::default(), "consensus", s).unwrap()).collect();
    let mut keys = Vec::new();
    while let Some(p) = g.to_move() {
        let l = legal(&g);
        let a = if l.len() == 1 { l[0] } else { bots[p].choose(&g, &l) };
        g.step(a, &mut keys).unwrap();
    }
    for s in 0..4u8 {
        let pool: Vec<_> = g.pool(s).iter().map(|&c| cube.def_of(c)).collect();
        let (mut want, _) = pools::build_deck(cube.db, &pool, &DeckGen::default());
        want.sort_unstable();
        assert_eq!(g.deck_and_sideboard(s).unwrap().0, want);
    }
}

/// Seat `builder()`'s whole build: `card(g, legal)` answers a choice of the
/// next pool card (or `DONE`), `count(legal)` a basic count; returns the
/// options of each decision.
fn build_seat(g: &mut TeamDraftAz, card: impl Fn(&TeamDraftAz, &[u16]) -> u16, count: impl Fn(&[u16]) -> u16) -> Vec<Vec<u16>> {
    let s = g.builder();
    let mut keys = Vec::new();
    let mut asked = Vec::new();
    while g.to_move() == Some(s as usize) {
        let l = legal(g);
        let a = if g.build(s).unwrap().cards_done() { count(&l) } else { card(g, &l) };
        asked.push(l);
        g.step(a, &mut keys).unwrap();
    }
    asked
}

/// The identities of the builder's pool not in its deck, sorted (the cards it
/// may add).
fn left(g: &TeamDraftAz) -> Vec<u16> {
    let b = g.build(g.builder()).unwrap();
    (0..b.order.len()).filter(|&j| b.inn & (1 << j) == 0).map(|j| b.order[j]).collect()
}

#[test]
fn deckbuilding_adds_any_card_until_done() {
    // ENGINE.md 47: each decision adds any pool card not in the deck (its
    // identity) or `DONE`, offered once the deck holds `deck_min` (22) pool
    // cards; a full deck (40 pool cards) ends the cards. Then the counts
    // (ENGINE.md 42: exactly 40 cards, at most 31 of a basic land; the Forest
    // count forced). 45-card pools of a 2v2 event.
    let m = DECK_MIN as usize;
    assert_eq!(m, 22);
    let mut g = new(4, 5);
    let mut keys = Vec::new();
    while g.phase() == Phase::Draft {
        let l = legal(&g);
        g.step(l[0], &mut keys).unwrap();
    }
    let single = |asked: &[Vec<u16>]| asked.iter().map(|l| l.len() == 1).collect::<Vec<_>>();
    let smallest = |_: &TeamDraftAz, l: &[u16]| -> u16 { l[0] };
    // Seat 0: the 22 smallest identities, DONE, the smallest counts: 18 basic
    // lands. Every card not in the deck is offered; DONE from 22 pool cards
    // on, an error before; a card already in the deck is an error.
    for t in 0..=m {
        let l = legal(&g);
        let mut want = left(&g);
        if t < m {
            assert_eq!(l, want, "{t} cards in: any card not in the deck");
            assert!(g.clone().step(DONE, &mut keys).is_err(), "DONE before the minimum");
            if t > 0 {
                let inn = g.build(0).unwrap().order[0];
                assert!(g.clone().step(inn, &mut keys).is_err(), "a card already in the deck");
            }
            g.step(l[0], &mut keys).unwrap();
        } else {
            want.push(DONE);
            assert_eq!(l, want, "the minimum reached: any card left, or DONE");
            g.step(DONE, &mut keys).unwrap();
        }
    }
    let asked = build_seat(&mut g, smallest, |l| l[0]);
    assert_eq!(single(&asked), [false, false, false, false, true], "the counts; Forest forced");
    let b = g.build(0).unwrap();
    assert_eq!((b.size(), b.pool_cards(), b.basics, b.inn), (40, m, [0, 0, 0, 0, 18], (1u64 << m) - 1));
    assert_eq!(g.deck_and_sideboard(0).unwrap().1.len(), 23);
    // Seat 1: the largest identities first, DONE at 22, the largest counts:
    // Plains 18, the rest 0 (forced).
    let asked = build_seat(&mut g, |g, l| if g.build(1).unwrap().pool_cards() == 22 { DONE } else { l[l.len() - 1] }, |l| *l.last().unwrap());
    assert_eq!(asked.len(), m + 6, "22 cards, DONE, 5 counts");
    assert_eq!(asked[0].len(), 45);
    assert_eq!(single(&asked[m + 1..]), [false, true, true, true, true], "Plains 18, then 0 forced");
    let b = g.build(1).unwrap();
    assert_eq!((b.size(), b.pool_cards(), b.basics, b.inn), (40, m, [18, 0, 0, 0, 0], ((1u64 << m) - 1) << 23));
    // Seat 2: every card: the 40th fills the deck and ends the cards (no
    // DONE asked; five cards in the sideboard); every count forced to 0.
    let asked = build_seat(&mut g, smallest, |l| l[0]);
    assert_eq!(asked.len(), 45);
    assert!(asked[..m].iter().all(|l| !l.contains(&DONE)));
    assert!(asked[m..40].iter().all(|l| l.last() == Some(&DONE)), "DONE offered from 22 pool cards on");
    assert!(asked[40..].iter().all(|l| l == &[COUNT]), "every count forced to 0");
    let b = g.build(2).unwrap();
    assert_eq!((b.size(), b.basics, b.pool_cards()), (40, [0; 5], 40));
    let (deck, side) = g.deck_and_sideboard(2).unwrap();
    assert_eq!((deck.len(), side.len()), (40, 5));
    // Seat 3: the odd positions in a shuffled order, DONE at 22, middle
    // counts. The same cards in another order are the same state.
    let order = g.build(3).unwrap().order.clone();
    let mut want: Vec<u16> = (0..m).map(|i| order[2 * i + 1]).collect();
    let mut rng = Rng::new(3);
    rng.shuffle(&mut want);
    let mut h = g.clone();
    for (i, &c) in want.iter().enumerate() {
        g.step(c, &mut keys).unwrap();
        h.step(want[want.len() - 1 - i], &mut keys).unwrap();
    }
    assert_eq!(g.build(3), h.build(3), "a deck is a set: one state whatever the order");
    assert_eq!(enc(&g, 3), enc(&h, 3));
    assert_eq!(legal(&g), legal(&h));
    let asked = build_seat(&mut g, |_, _| DONE, |l| l[l.len() / 2]);
    assert_eq!(asked.len(), 6, "DONE, 5 counts");
    let b = g.build(3).unwrap();
    assert_eq!((b.size(), b.pool_cards(), b.basics), (40, m, [9, 5, 2, 1, 1]));
    assert_eq!(b.inn, (0..m).fold(0u64, |x, i| x | 1 << (2 * i + 1)));
    assert_eq!(g.deck_and_sideboard(3).unwrap().1.len(), 23);
    g.check_invariants().unwrap();
    assert!(g.deferred());
}

/// `seat`'s card choices from its first decision, depth first: the deck bits
/// of every end of the cards (one per path), and (decisions, single options,
/// DONE offered). Checks DONE's legality at every decision.
fn card_paths(g: &TeamDraftAz, seat: u8) -> (Vec<u64>, [u64; 3]) {
    let m = g.ev.deck_min as usize;
    let mut keys = Vec::new();
    let mut ends = Vec::new();
    let mut n = [0u64; 3];
    let mut stack = vec![g.clone()];
    while let Some(g) = stack.pop() {
        let b = g.build(seat).unwrap();
        if b.cards_done() {
            ends.push(b.inn);
            continue;
        }
        let l = legal(&g);
        assert!(!l.is_empty(), "a dead end");
        assert_eq!(l.contains(&DONE), b.pool_cards() >= m, "DONE offered iff the deck holds the minimum");
        n[0] += 1;
        n[1] += (l.len() == 1) as u64;
        n[2] += l.contains(&DONE) as u64;
        for &a in &l {
            let mut c = g.clone();
            c.step(a, &mut keys).unwrap();
            stack.push(c);
        }
    }
    (ends, n)
}

/// `seat`'s card choices as states (a deck is a set: the states of every
/// order merged): the deck bits of every end state, and the states seen.
fn card_states(g: &TeamDraftAz, seat: u8) -> (Vec<u64>, usize) {
    let m = g.ev.deck_min as usize;
    let mut keys = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut ends = Vec::new();
    let mut stack = vec![g.clone()];
    while let Some(g) = stack.pop() {
        let b = g.build(seat).unwrap();
        if !seen.insert((b.inn, b.cards_done())) {
            continue;
        }
        if b.cards_done() {
            ends.push(b.inn);
            continue;
        }
        let l = legal(&g);
        assert_eq!(l.contains(&DONE), b.pool_cards() >= m);
        for &a in &l {
            let mut c = g.clone();
            c.step(a, &mut keys).unwrap();
            stack.push(c);
        }
    }
    (ends, seen.len())
}

fn binomial(n: u64, k: u64) -> u64 {
    (0..k).fold(1, |acc, i| acc * (n - i) / (i + 1))
}

/// A 1v1 event of one pack per seat at its first deckbuilding decision.
fn at_deckbuilding(sp: &str) -> TeamDraftAz {
    let mut g = TeamDraftAz::new(&spec(sp), 2, 7).unwrap();
    let mut keys = Vec::new();
    while g.phase() == Phase::Draft {
        let l = legal(&g);
        g.step(l[0], &mut keys).unwrap();
    }
    g
}

#[test]
fn every_legal_deck_is_reachable_and_one_state() {
    // ENGINE.md 47: a legal deck is a set of pool cards (at least the
    // minimum, at most 40) and basic lands making 40 (at most 31 of a type);
    // the cards are added in any order, so a deck of k pool cards is reached
    // by its k! orders, all ending in the same state.
    // (a) Every path of the card choices: 6- and 7-card pools for several
    // minimums: each set of at least the minimum ends exactly k! paths, no
    // other set ends any.
    let fact = |k: u64| (1..=k).product::<u64>();
    for (pack_size, min) in [(6u8, 0u8), (6, 1), (6, 4), (6, 6), (7, 3)] {
        let g = at_deckbuilding(&format!(r#"{{"packs": 1, "pack_size": {pack_size}, "deck_min": {min}}}"#));
        let n = pack_size as u64;
        let (ends, [decisions, forced, done]) = card_paths(&g, 0);
        let mut by = std::collections::HashMap::new();
        for &s in &ends {
            *by.entry(s).or_insert(0u64) += 1;
        }
        for (&s, &paths) in &by {
            let k = s.count_ones() as u64;
            assert!(k >= min as u64, "a set below the minimum");
            assert_eq!(paths, fact(k), "set {s:#x}: {paths} paths, {k}! orders");
        }
        let sets: u64 = (min as u64..=n).map(|k| binomial(n, k)).sum();
        let paths: u64 = (min as u64..=n).map(|k| binomial(n, k) * fact(k)).sum();
        println!("{n}-card pool, minimum {min}: {} card sets of {sets}, {} paths of {paths}, {decisions} decisions ({forced} single options, DONE offered at {done})", by.len(), ends.len());
        assert_eq!((by.len() as u64, ends.len() as u64), (sets, paths));
    }
    // (b) The states (orders merged): 12- and 15-card pools: every set of at
    // least the minimum is one end state, no other. (The 40-card cap: the
    // extremes above and the fuzz.)
    for (pack_size, packs, min) in [(12u8, 1u8, 0u8), (12, 1, 5), (12, 1, 12), (15, 1, 3)] {
        let g = at_deckbuilding(&format!(r#"{{"packs": {packs}, "pack_size": {pack_size}, "deck_min": {min}}}"#));
        let n = packs as u64 * pack_size as u64;
        let t0 = std::time::Instant::now();
        let (ends, states) = card_states(&g, 0);
        let mut u = ends.clone();
        u.sort_unstable();
        u.dedup();
        assert_eq!(u.len(), ends.len());
        assert!(ends.iter().all(|s| (min as u32..=40).contains(&s.count_ones())));
        let want: u64 = (min as u64..=n.min(40)).map(|k| binomial(n, k)).sum();
        println!("{n}-card pool, minimum {min}: {} end states of {want}, {states} states, {:.1} s", ends.len(), t0.elapsed().as_secs_f64());
        assert_eq!(ends.len() as u64, want);
    }
    // (c) Whole decks: a two-card pool, minimums 0, 1 and 2: every deck (its
    // cards and five counts of 0..=31 summing to 40 - its cards) is an end
    // state, by k! paths.
    let comps = |k: u32| -> u64 {
        let mut n = 0u64;
        for a in 0..=31u32 {
            for b in 0..=31u32 {
                for c in 0..=31u32 {
                    for d in 0..=31u32 {
                        let e = 40i64 - k as i64 - (a + b + c + d) as i64;
                        n += (0..=31).contains(&e) as u64;
                    }
                }
            }
        }
        n
    };
    for min in 0..=2u8 {
        let g = at_deckbuilding(&format!(r#"{{"packs": 1, "pack_size": 2, "deck_min": {min}}}"#));
        assert_eq!(g.build(0).unwrap().order.len(), 2);
        let mut keys = Vec::new();
        let mut seen = std::collections::HashMap::new();
        let (mut leaves, mut nodes, mut forced) = (0u64, 0u64, 0u64);
        let mut stack = vec![g];
        while let Some(g) = stack.pop() {
            if g.builder() != 0 {
                let b = g.build(0).unwrap();
                assert_eq!(b.size(), DECK_SIZE);
                assert!(b.pool_cards() >= min as usize);
                assert!(b.basics.iter().all(|&x| x <= MAX_BASIC));
                let key = b.inn | b.basics.iter().enumerate().fold(0u64, |k, (j, &x)| k | (x as u64) << (8 + 6 * j));
                *seen.entry(key).or_insert(0u64) += 1;
                leaves += 1;
                continue;
            }
            nodes += 1;
            let l = legal(&g);
            assert!(!l.is_empty(), "a dead end");
            forced += (l.len() == 1) as u64;
            for &a in &l {
                let mut c = g.clone();
                c.step(a, &mut keys).unwrap();
                stack.push(c);
            }
        }
        for (&key, &paths) in &seen {
            assert_eq!(paths, fact((key & 3).count_ones() as u64));
        }
        let want: u64 = (min as u32..=2).map(|k| [1, 2, 1][k as usize] * comps(k)).sum();
        println!("two-card pool, minimum {min}: {} decks, {leaves} paths, {nodes} decisions, {forced} forced (single option)", seen.len());
        assert_eq!(seen.len() as u64, want);
    }
}

#[test]
fn pairings_are_rotations_of_a_latin_square() {
    let mut seen_rot = std::collections::BTreeSet::new();
    for seed in 0..40 {
        let mut g = new(8, seed);
        let mut rng = Rng::new(seed);
        finish_random(&mut g, &mut rng);
        let m = g.matches().unwrap();
        assert_eq!(m.len(), 12);
        let rot = g.rotations().to_vec();
        assert_eq!(rot.len(), 3);
        let mut r2 = rot.clone();
        r2.sort_unstable();
        r2.dedup();
        assert_eq!(r2.len(), 3, "distinct rotations");
        seen_rot.insert(rot);
        for round in 0..3u8 {
            let ms: Vec<_> = m.iter().filter(|x| x.round == round).collect();
            assert_eq!(ms.len(), 4);
            let mut a: Vec<u8> = ms.iter().map(|x| x.seats[0]).collect();
            let mut b: Vec<u8> = ms.iter().map(|x| x.seats[1]).collect();
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, vec![0, 2, 4, 6]);
            assert_eq!(b, vec![1, 3, 5, 7]);
        }
        for s in [0u8, 2, 4, 6] {
            let mut opp: Vec<u8> = m.iter().filter(|x| x.seats[0] == s).map(|x| x.seats[1]).collect();
            assert_eq!(opp.len(), 3);
            opp.sort_unstable();
            opp.dedup();
            assert_eq!(opp.len(), 3, "three different opponents");
        }
        for x in &m {
            assert_eq!(x.players[0].seat, x.seats[0]);
            assert_eq!(x.players[1].seat, x.seats[1]);
        }
    }
    assert!(seen_rot.len() > 10, "{}", seen_rot.len());
    // 2v2: both rotations (4 matches); 1v1: one; a set number of rounds.
    let mut rng = Rng::new(2);
    for (players, js, n) in [(4usize, "{}", 4usize), (2, "{}", 1), (8, r#"{"rounds": 1}"#, 4), (6, "{}", 9)] {
        let mut g = TeamDraftAz::new(&spec(js), players, 3).unwrap();
        finish_random(&mut g, &mut rng);
        assert_eq!(g.matches().unwrap().len(), n, "{players} {js}");
    }
    assert!(TeamDraftAz::new(&spec(r#"{"rounds": 3}"#), 4, 1).is_err());
}

#[test]
fn after_the_draft_a_team_knows_every_card_but_the_opposing_first_picks() {
    let mut rng = Rng::new(3);
    for players in [8usize, 4, 2] {
        let mut g = new(players, 11);
        let mut keys = Vec::new();
        while g.phase() == Phase::Draft {
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
        let ev = g.ev.clone();
        let n = ev.n as usize;
        for team in 0..2u8 {
            let cells = g.known_cells(ev.team_mask(team));
            let mut want = Vec::new();
            for p in 0..ev.n_packs() {
                for t in 0..15u8 {
                    let opener_is_opp = (p % n) as u8 % 2 != team;
                    if !(t == 0 && opener_is_opp) {
                        want.push((p as u8, t));
                    }
                }
            }
            assert_eq!(cells, want, "{players} seats, team {team}");
            // The cards it knows: all drafted but the other team's 3 T first picks.
            let known = g.team_known(team);
            assert_eq!(known.len(), 45 * players - 3 * players / 2);
            for o in (0..ev.n).filter(|o| o % 2 != team) {
                for c in g.first_picks(o) {
                    assert!(known.binary_search(&c).is_err());
                }
            }
            // The keys of the draft's end: every cell the team knows.
            let tk: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_TEAM && key_player(k) == team).collect();
            assert_eq!(tk.len(), want.len());
        }
        // During the draft a seat knows only its own picks exactly.
        let mut h = new(players, 12);
        for _ in 0..(players * 4) {
            let l = legal(&h);
            h.step(l[0], &mut keys).unwrap();
        }
        for s in 0..ev.n {
            let cells = h.known_cells(1 << s);
            for (p, t) in cells {
                let mine = ev.seat_at(p as usize, t as usize) == s;
                assert!(mine || players == 2, "seat {s} knows another's pick exactly ({p}, {t})");
            }
        }
    }
}

#[test]
fn determinize_keeps_what_the_viewer_knows() {
    let mut zrng = zr(5);
    let mut rng = Rng::new(5);
    let (mut checks, mut changed) = (0, 0);
    for players in [8usize, 4, 2] {
        for g in states(players, scale(6), 25, 5 + players as u64) {
            let d = g.to_move().unwrap() as u8;
            let other = rng.below(players as u32) as u8;
            for v in [d, other] {
                let mut w = g.clone();
                w.determinize(v as usize, &mut zrng);
                w.check_invariants().unwrap();
                let mask = g.knowing(v);
                assert_eq!(w.knowledge(mask), g.knowledge(mask), "the viewer's knowledge changed");
                assert_eq!(w.picks(v), g.picks(v));
                assert_eq!(enc(&w, v), enc(&g, v), "the viewer's features changed (viewer {v}, decider {d})");
                assert_eq!(w.to_move(), g.to_move());
                if v == d {
                    assert_eq!(legal(&w), legal(&g), "the decider's options changed");
                }
                if g.phase() != Phase::Draft {
                    // Teammates' pools and builds are known.
                    for m in (0..players as u8).filter(|m| m % 2 == v % 2) {
                        assert_eq!(w.pool(m), g.pool(m));
                        assert_eq!(w.build(m), g.build(m));
                    }
                }
                checks += 1;
                changed += (0..g.ev.n).any(|s| w.pool(s) != g.pool(s)) as u32;
            }
        }
    }
    println!("determinize checks {checks}, worlds with other pools {changed}");
    assert!(checks > 200, "{checks}");
    assert!(changed * 2 > checks, "{changed}/{checks}");
}

#[test]
fn equal_keys_mean_equal_features() {
    let mut zrng = zr(6);
    let mut rng = Rng::new(6);
    let mut compared = 0;
    for players in [8usize, 4, 2] {
        for g in states(players, scale(5), 20, 60 + players as u64) {
            let viewer = g.to_move().unwrap() as u8;
            let (mut a, mut b) = (g.clone(), g.clone());
            a.determinize(viewer as usize, &mut zrng);
            b.determinize(viewer as usize, &mut zrng);
            for _ in 0..60 {
                let (la, lb) = (legal(&a), legal(&b));
                if a.to_move() != b.to_move() || la != lb {
                    assert!(!(a.to_move() == Some(viewer as usize) && b.to_move() == Some(viewer as usize)), "the viewer's options differ");
                    break;
                }
                assert_eq!(enc(&a, viewer), enc(&b, viewer), "same keys and decision, different features");
                compared += 1;
                if a.to_move().is_none() {
                    break;
                }
                let x = la[rng.below(la.len() as u32) as usize];
                let (mut ka, mut kb) = (Vec::new(), Vec::new());
                a.step(x, &mut ka).unwrap();
                b.step(x, &mut kb).unwrap();
                assert_eq!(ka.len(), kb.len(), "key counts differ between worlds");
                if ka != kb {
                    break;
                }
            }
        }
    }
    println!("compared {compared}");
    assert!(compared > 500, "{compared}");
}

#[test]
fn keys_are_masked_for_the_observer() {
    let mut g = new(8, 13);
    let mut keys = Vec::new();
    for _ in 0..7 {
        let l = legal(&g);
        g.step(l[0], &mut keys).unwrap();
    }
    let mut w = g.clone();
    w.determinize(7, &mut zr(1));
    let l = legal(&w);
    w.step(l[0], &mut keys).unwrap();
    // Seat 7's pack (14 cards), the others one HIDDEN key each.
    assert_eq!(keys.len(), 14 + 7);
    for &k in &keys {
        assert_eq!(key_kind(k), KEY_SEE);
        assert_eq!(key_payload(k) == HIDDEN, key_player(k) != 7);
    }
}

#[test]
fn clone_continues_identically() {
    let mut rng = Rng::new(7);
    for g in states(8, scale(3), 60, 7) {
        let mut a = g.clone();
        let mut b = g.clone();
        let mut r2 = Rng::new(rng.next_u64());
        let mut r3 = r2.clone();
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        while a.to_move().is_some() {
            let la = legal(&a);
            let lb = legal(&b);
            assert_eq!(la, lb);
            let x = la[r2.below(la.len() as u32) as usize];
            let y = lb[r3.below(lb.len() as u32) as usize];
            a.step(x, &mut ka).unwrap();
            b.step(y, &mut kb).unwrap();
            assert_eq!(ka, kb);
        }
        assert_eq!(a.matches().unwrap(), b.matches().unwrap());
        assert_eq!(enc(&a, 0), enc(&b, 0));
    }
}

/// A state where the viewer's transition opens the next round's packs.
fn before_openings(players: usize, seed: u64) -> TeamDraftAz {
    let mut g = new(players, seed);
    let mut rng = Rng::new(seed);
    let mut keys = Vec::new();
    let n = players as u8;
    loop {
        let (r, p, t) = g.pick_round();
        if r < 2 && p == 13 && t == n - 1 {
            return g;
        }
        let l = legal(&g);
        g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
    }
}

#[test]
fn force_chance_replays_the_viewers_openings() {
    let mut zrng = zr(8);
    let (mut tried, mut hits, mut whole) = (0, 0, 0);
    let opening = |k: &[u64], v: u8| -> Vec<u64> {
        let m: Vec<u64> = k.iter().copied().filter(|&x| key_kind(x) == KEY_SEE && key_player(x) == v && key_payload(x) != HIDDEN).collect();
        m[m.len() - 15..].to_vec()
    };
    for e in 0..scale(20) {
        let players = [8usize, 4, 2][e as usize % 3];
        let g = before_openings(players, 80 + e);
        let viewer = g.to_move().unwrap() as u8;
        let x = legal(&g)[0];
        let mut a = g.clone();
        a.determinize(viewer as usize, &mut zrng);
        let mut ka = Vec::new();
        transition(&mut a, x, &mut ka);
        let mine: Vec<u64> = ka.iter().copied().filter(|&k| key_kind(k) == KEY_SEE && key_player(k) == viewer && key_payload(k) != HIDDEN).collect();
        assert!(mine.len() >= 15, "the viewer's opening is keyed");
        for _ in 0..4 {
            let mut b = g.clone();
            b.determinize(viewer as usize, &mut zrng);
            let before = b.knowledge(1 << viewer);
            b.force_chance(&ka);
            let mut kb = Vec::new();
            transition(&mut b, x, &mut kb);
            b.force_chance(&[]);
            b.check_invariants().unwrap();
            assert_eq!(kb.len(), ka.len());
            tried += 1;
            hits += (opening(&kb, viewer) == opening(&ka, viewer)) as u32;
            // (The transition also shows the viewer the last card of a pack in
            // flight: a revelation of other seats' picks, never forced.)
            if kb == ka {
                whole += 1;
                assert_eq!(enc(&a, viewer), enc(&b, viewer), "forced to the same keys, different features");
            }
            // What the viewer had seen is unchanged.
            let after = b.knowledge(1 << viewer);
            for y in &before {
                let x = after.iter().find(|x| x.0 == y.0 && x.1 == y.1).expect("a view is never lost");
                assert_eq!(x.2, y.2, "forcing changed what the viewer had seen");
            }
        }
    }
    println!("forced openings {hits}/{tried}, whole transitions {whole}");
    assert!(tried >= 60 && whole > 0);
    assert_eq!(hits, tried, "the viewer's own opening always replays");
}

#[test]
fn forced_keys_are_consumed_in_order_and_impossible_ones_fall_back() {
    let mut zrng = zr(9);
    let g = before_openings(8, 90);
    let viewer = g.to_move().unwrap() as u8;
    let x = legal(&g)[0];
    let mut a = g.clone();
    a.determinize(viewer as usize, &mut zrng);
    let mut ka = Vec::new();
    transition(&mut a, x, &mut ka);
    // An impossible card for the viewer's opening: one of its own picks.
    let own = g.picks(viewer)[0].0 as u64;
    let mine = |k: &[u64]| -> Vec<usize> {
        (0..k.len()).filter(|&i| key_kind(k[i]) == KEY_SEE && key_player(k[i]) == viewer && key_payload(k[i]) != HIDDEN).collect()
    };
    let ia = mine(&ka);
    let open = &ia[ia.len() - 15..];
    let mut forced = ka.clone();
    forced[open[14]] = key(KEY_SEE, viewer, own);
    let mut b = g.clone();
    b.determinize(viewer as usize, &mut zrng);
    b.force_chance(&forced);
    let mut kb = Vec::new();
    transition(&mut b, x, &mut kb);
    b.force_chance(&[]);
    b.check_invariants().unwrap();
    assert_eq!(kb.len(), ka.len(), "every key consumed one queued key");
    let ib = mine(&kb);
    let got: Vec<u64> = ib[ib.len() - 15..].iter().map(|&i| kb[i]).collect();
    assert!(!got.iter().any(|&k| key_payload(k) == own), "an impossible card was forced");
    // The other forced cards of the opening still landed.
    let n_hit = open[..14].iter().filter(|&&i| got.contains(&ka[i])).count();
    assert_eq!(n_hit, 14, "the possible forced cards are in the pack");
}

#[test]
fn the_heuristic_matches_resolve_an_event() {
    let mut rng = Rng::new(10);
    for (players, seed) in [(8usize, 1u64), (4, 2), (2, 3)] {
        let mut g = new(players, 100 + seed);
        let mut bots: Vec<_> = (0..players).map(|s| make_bot::<TeamDraftAz>(&DraftSpec::default(), "consensus", s as u64).unwrap()).collect();
        let mut keys = Vec::new();
        while let Some(p) = g.to_move() {
            let l = legal(&g);
            // consensus for team A, random for team B
            let a = if l.len() == 1 { l[0] } else if p % 2 == 0 { bots[p].choose(&g, &l) } else { l[rng.below(l.len() as u32) as usize] };
            g.step(a, &mut keys).unwrap();
        }
        assert!(g.deferred());
        let payload: serde_json::Value = serde_json::from_str(&g.deferred_payload().unwrap()).unwrap();
        let n_matches = payload["matches"].as_array().unwrap().len();
        let t0 = std::time::Instant::now();
        let res = draft_bots::resolve_with_heuristic(&mut g, seed).unwrap();
        let secs = t0.elapsed().as_secs_f64();
        assert_eq!(res.len(), n_matches);
        assert!(!g.deferred());
        let o = g.outcome().unwrap().clone();
        let r = g.result();
        assert!((r.iter().sum::<f32>() - 1.0).abs() < 1e-6, "{r:?}");
        for s in 0..players {
            assert_eq!(r[s], r[s % 2], "teammates share the outcome");
            let m = g.margin(s);
            assert!((-1.0..=1.0).contains(&m));
            assert!((m - (o.wins[s % 2] as f32 - o.wins[1 - s % 2] as f32) / n_matches as f32).abs() < 1e-6);
        }
        let t = (players / 2) as f32;
        assert!([1.0 / t, 0.0, 0.5 / t].contains(&r[0]));
        for x in &res {
            assert!(x.games >= 1 && x.games <= 3);
            assert!((x.result[0] + x.result[1] - 1.0).abs() < 1e-6);
        }
        println!(
            "{players} seats: {} matches in {secs:.2} s, team match wins {:?}, result {:?}, games {:?}",
            res.len(),
            o.wins,
            o.team,
            res.iter().map(|x| x.games).collect::<Vec<_>>()
        );
        assert!(g.resolve(&o.results).is_err(), "resolved twice");
    }
}

#[test]
#[should_panic(expected = "deferred result")]
fn the_result_is_unknown_while_the_matches_are_pending() {
    let mut g = new(4, 1);
    finish_random(&mut g, &mut Rng::new(1));
    assert!(g.deferred() && g.to_move().is_none());
    let _ = g.result();
}

#[test]
fn resolve_checks_its_results() {
    let mut g = new(4, 2);
    assert!(g.resolve(&[[1.0, 0.0]; 4]).is_err(), "not at the terminal");
    finish_random(&mut g, &mut Rng::new(2));
    assert!(g.resolve(&[[1.0, 0.0]; 3]).is_err(), "too few");
    assert!(g.resolve(&[[1.0, 1.0]; 4]).is_err(), "not a result");
    let mut h = g.clone();
    h.resolve_json(r#"[[1,0],[0.5,0.5],{"result":[0,1]},[0,1]]"#).unwrap();
    assert_eq!(h.outcome().unwrap().wins, [1, 2]);
    assert_eq!(h.result()[1], 0.5);
    assert_eq!(h.result()[0], 0.0);
    assert!((h.margin(1) - 0.25).abs() < 1e-6);
    let mut k = g.clone();
    k.resolve(&[[1.0, 0.0], [0.0, 1.0], [0.5, 0.5], [0.5, 0.5]]).unwrap();
    assert_eq!(k.result()[..4], [0.25; 4]);
    assert_eq!(k.margin(0), 0.0);
}

#[test]
fn the_payload_describes_every_match() {
    let cube = cube().unwrap();
    let mut g = new(8, 21);
    finish_random(&mut g, &mut Rng::new(21));
    let ms = g.matches().unwrap();
    for m in &ms {
        for (i, p) in m.players.iter().enumerate() {
            let o = m.seats[1 - i];
            // first picks: the player's own, one per pack
            let fp: Vec<_> = g.first_picks(p.seat).iter().map(|&c| cube.def_of(c)).collect();
            let mut fp = fp;
            fp.sort_unstable();
            assert_eq!(p.first_picks, fp);
            // elsewhere: what the team saw, none of it in the opponent's known pool,
            // none of it the opponent's first picks
            let opp_pool: Vec<_> = g.pool(o).iter().map(|&c| cube.def_of(c)).collect();
            let opp_fp: Vec<_> = g.first_picks(o).iter().map(|&c| cube.def_of(c)).collect();
            for d in &p.elsewhere {
                assert!(!opp_pool.contains(d) && !opp_fp.contains(d));
            }
            assert_eq!(p.elsewhere.len(), 360 - 12 - (45 - 3));
            assert!(p.deck.len() >= 40);
        }
    }
    let v: serde_json::Value = serde_json::from_str(&g.deferred_payload().unwrap()).unwrap();
    assert_eq!(v["matches"].as_array().unwrap().len(), 12);
    assert_eq!(v["event"]["team_size"], 4);
    let p0 = &v["matches"][0]["players"][0];
    assert!(p0["deck"].as_array().unwrap().len() >= 40);
    assert_eq!(p0["first_picks"].as_array().unwrap().len(), 3);
}

#[test]
fn encoding_slots_hold_the_pack_and_the_picks() {
    let cube = cube().unwrap();
    let ident = |x: &[f32]| -> u16 {
        let a = x[..16].iter().position(|&v| v == 1.0).unwrap();
        let b = x[16..24].iter().position(|&v| v == 1.0).unwrap();
        let c = x[24..32].iter().position(|&v| v == 1.0).unwrap();
        (a * 64 + b * 8 + c) as u16
    };
    use draft_encode::{CODE, O_PACK, O_POOL, PACK_W, POOL_W};
    let mut builds = 0;
    for g in states(8, 2, 30, 31) {
        let v = g.to_move().unwrap() as u8;
        let x = enc(&g, v);
        assert!(x.iter().all(|f| f.is_finite()));
        let l = legal(&g);
        if let Some(p) = g.pack_held(v) {
            let pk = g.pack(p);
            for (k, &c) in pk.iter().enumerate() {
                let o = O_PACK + k * PACK_W;
                assert_eq!(ident(&x[o..o + CODE]), c);
                assert_eq!(x[o + CODE], 1.0);
                assert_eq!(x[o + CODE + 1], 1.0, "every card of my pack is an option");
            }
            if pk.len() < draft::MAX_PACK {
                assert_eq!(x[O_PACK + pk.len() * PACK_W + CODE], 0.0);
            }
            assert_eq!(pk, &l[..]);
        }
        for (k, &(c, _, _)) in g.picks(v).iter().enumerate() {
            let o = O_POOL + k * POOL_W;
            assert_eq!(ident(&x[o..o + CODE]), c);
            assert_eq!(x[o + CODE], 1.0);
        }
        if g.phase() == Phase::Build && !g.build(v).unwrap().cards_done() {
            // Choosing the next card: every card not in the deck is still
            // addable and an option of its pool slot; the cards in are IN,
            // none OUT (ENGINE.md 47).
            use draft_encode::{gl, pool};
            let b = g.build(v).unwrap();
            for (k, &(c, _, _)) in g.picks(v).iter().enumerate() {
                let st = &x[O_POOL + k * POOL_W + CODE..O_POOL + (k + 1) * POOL_W];
                let i = b.order.iter().position(|&y| y == c).unwrap();
                let inn = b.inn & (1 << i) != 0;
                assert_eq!(st[pool::OPTION] == 1.0, l.contains(&c), "{}", cube.name(c));
                assert_eq!(st[pool::OPTION] == 1.0, !inn);
                assert_eq!(st[pool::AVAILABLE] == 1.0, !inn);
                assert_eq!((st[pool::IN] == 1.0, st[pool::OUT]), (inn, 0.0));
            }
            assert_eq!(x[gl::BUILD_CARD], 1.0);
            assert_eq!(x[gl::DONE_OK] == 1.0, l.contains(&DONE));
            assert_eq!(x[gl::NEED], b.need(g.ev.deck_min) as f32 / 40.0);
            assert_eq!(x[gl::POOL_IN], b.pool_cards() as f32 / 40.0);
            builds += 1;
        }
    }
    assert!(builds > 10, "{builds}");
}

#[test]
fn the_bots() {
    let st = draft_bots::pick_strength();
    let cube = cube().unwrap();
    assert!(cube.cards.iter().all(|&c| st[c as usize].is_finite()), "every draftable card has a pick strength");
    let black_lotus = cube.cards.iter().copied().find(|&c| cube.name(c) == "Black Lotus").unwrap();
    let bolt = cube.cards.iter().copied().find(|&c| cube.name(c) == "Lightning Bolt").unwrap();
    assert_eq!(Consensus::pick(&[bolt, black_lotus]), black_lotus);
    assert_eq!(<TeamDraftAz as SearchGame>::bot_names(), &["consensus"]);
    let mut g = new(8, 4);
    let mut b = make_bot::<TeamDraftAz>(&DraftSpec::default(), "consensus", 0).unwrap();
    let l = legal(&g);
    let a = b.choose(&g, &l);
    assert!(l.iter().all(|&c| st[c as usize] <= st[a as usize]));
    let mut r = make_bot::<TeamDraftAz>(&DraftSpec::default(), "random", 0).unwrap();
    let mut keys = Vec::new();
    while g.to_move().is_some() {
        let l = legal(&g);
        let a = if l.len() == 1 { l[0] } else { r.choose(&g, &l) };
        assert!(l.contains(&a));
        g.step(a, &mut keys).unwrap();
    }
    assert!(make_bot::<TeamDraftAz>(&DraftSpec::default(), "heuristic", 0).is_none());
}

#[test]
fn spec_parsing() {
    let s = spec("");
    assert_eq!((s.packs, s.pack_size, s.rounds, s.max_turns, s.deck_min), (3, 15, None, 60, 22));
    let s = spec(r#"{"rounds": 2, "packs": 2, "pack_size": 9, "max_turns": 40, "format": "bo1", "deck_min": 33}"#);
    assert_eq!((s.packs, s.pack_size, s.rounds, s.max_turns, s.deck_min), (2, 9, Some(2), 40, 33));
    // The minimum is at most the pool.
    assert_eq!(TeamDraftAz::new(&s, 4, 1).unwrap().ev.deck_min, 18);
    assert_eq!(TeamDraftAz::new(&spec(r#"{"deck_min": 40}"#), 4, 1).unwrap().ev.deck_min, 40);
    for bad in [
        r#"{"bogus": 1}"#,
        r#"{"packs": 4}"#,
        r#"{"pack_size": 16}"#,
        r#"{"pack_size": 1}"#,
        r#"{"rounds": 0}"#,
        r#"{"format": "bo5"}"#,
        r#"{"deck_min": 41}"#,
        r#"{"deck_min": -1}"#,
        "[1]",
    ] {
        assert!(DraftSpec::parse(bad).is_err(), "{bad}");
    }
    for players in [0usize, 1, 3, 10] {
        assert!(TeamDraftAz::new_game(&spec(""), players, 1).is_err(), "{players}");
    }
    assert_eq!(<TeamDraftAz as SearchGame>::NUM_ACTIONS, 1024);
    assert_eq!(<TeamDraftAz as SearchGame>::NUM_FEATURES, 14_492);
    assert_eq!(draft::DONE, 768);
}

#[test]
fn layout_json_is_checked_in() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../mtgz/az/layout_draft_v2.json");
    let want = draft_encode::layout_json();
    if std::env::var("MTGZ_WRITE_LAYOUT").is_ok() {
        std::fs::write(&path, &want).unwrap();
    }
    let got = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(got == want, "mtgz/az/layout_draft_v2.json is stale: MTGZ_WRITE_LAYOUT=1 cargo test -p engine --test draft layout");
}

/// The random-actions fuzz of whole events (`DRAFT_FUZZ=n` events, 4v4, 2v2,
/// 3v3 and 1v1; the deck minimum 20, 0, 40 or random): the invariants after
/// every action; the decider's encoding
/// finite and within `encode::FEATURE_BOUND` at every decision, and every
/// world's (ENGINE.md 46); every ~40 actions
/// `determinize` for the decider and another viewer (knowledge, features,
/// options, invariants); at one decision of every 8th event every offered
/// option completes the event; the event resolved with random results (every
/// 50th by heuristic-played matches: a match that panics in the `mtg` engine
/// is recorded with its spec, the event resolved with random results, and the
/// test fails at the end listing every such match).
#[test]
fn fuzz_random_events() {
    let events: u64 = std::env::var("DRAFT_FUZZ").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let mut rng = Rng::new(0xF022);
    let mut zrng = zr(0xF022);
    let (mut steps, mut worlds, mut options_done, mut searched) = (0u64, 0u64, 0u64, [0u64; 2]);
    let mut team_wins = [0u64; 3];
    let mut heuristic_matches = 0u64;
    let mut match_panics: Vec<String> = Vec::new();
    // decks, their pool cards, card choices (DONE offered at) by minimum: the default / other
    let mut decks = [[0u64; 4]; 2];
    let t0 = std::time::Instant::now();
    for e in 0..events {
        let players = [8usize, 4, 8, 2, 8, 6][e as usize % 6];
        let min = [DECK_MIN as u32, 0, 40, rng.below(41)][(e / 6) as usize % 4];
        let mut g = TeamDraftAz::new(&spec(&format!(r#"{{"deck_min": {min}}}"#)), players, 0x5EED_0000 + e).unwrap();
        // (an event is 45 picks and 25-46 deckbuilding decisions per seat)
        let check_at = if e % 8 == 0 { rng.below(80 * players as u32) as u64 } else { u64::MAX };
        let mut i = 0u64;
        let mut keys = Vec::new();
        let mut x = vec![0.0f32; NUM_FEATURES];
        while let Some(d) = g.to_move() {
            let l = legal(&g);
            assert!(!l.is_empty() && l.windows(2).all(|w| w[0] < w[1]) && l.iter().all(|&a| (a as usize) < 1024));
            x.fill(0.0);
            g.encode(d, &mut x);
            assert_bounded(&x, e, i, d as u8);
            if l.len() >= 2 {
                searched[(g.phase() == Phase::Build) as usize] += 1;
            }
            if g.phase() == Phase::Build && !g.build(d as u8).unwrap().cards_done() {
                decks[(min != DECK_MIN as u32) as usize][2] += 1;
                decks[(min != DECK_MIN as u32) as usize][3] += l.contains(&DONE) as u64;
                assert_eq!(l.contains(&DONE), g.build(d as u8).unwrap().pool_cards() >= min as usize);
            }
            if i % 40 == 17 {
                for v in [d as u8, rng.below(players as u32) as u8] {
                    let mut w = g.clone();
                    w.determinize(v as usize, &mut zrng);
                    w.check_invariants().unwrap_or_else(|err| panic!("event {e}: world for {v}: {err}"));
                    let mask = g.knowing(v);
                    assert_eq!(w.knowledge(mask), g.knowledge(mask));
                    let xw = enc(&w, v);
                    assert_bounded(&xw, e, i, v);
                    assert_eq!(xw, enc(&g, v), "event {e} step {i}: viewer {v}");
                    assert_eq!(w.to_move(), g.to_move());
                    if v == d as u8 {
                        assert_eq!(legal(&w), l);
                    }
                    // the world plays on to its terminal
                    if rng.below(8) == 0 {
                        finish_random(&mut w, &mut rng);
                        w.check_invariants().unwrap();
                    }
                    worlds += 1;
                }
            }
            if i == check_at {
                for &a in &l {
                    let mut w = g.clone();
                    w.step(a, &mut keys).unwrap();
                    finish_random(&mut w, &mut rng);
                    w.check_invariants().unwrap();
                    assert!(w.deferred());
                    options_done += 1;
                }
            }
            let a = l[rng.below(l.len() as u32) as usize];
            keys.clear();
            g.step(a, &mut keys).unwrap();
            g.check_invariants().unwrap_or_else(|err| panic!("event {e} step {i}: {err}"));
            i += 1;
        }
        steps += i;
        assert!(g.deferred());
        for s in 0..players as u8 {
            let b = g.build(s).unwrap();
            assert!(b.pool_cards() >= min as usize && b.size() == DECK_SIZE);
            decks[(min != DECK_MIN as u32) as usize][0] += 1;
            decks[(min != DECK_MIN as u32) as usize][1] += b.pool_cards() as u64;
        }
        let n = g.matches().unwrap().len();
        if e % 50 == 7 {
            // Random decks are playable: the heuristic plays the event's matches
            // (one at a time: a panic of the `mtg` engine is recorded with the
            // match's spec, and fails the test at the end).
            let (max_turns, format) = (g.ev.max_turns, g.ev.format);
            let mut res = Vec::new();
            for (i, setup) in g.matches().unwrap().iter().enumerate() {
                let played = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    draft_bots::play_match_heuristic(setup, max_turns, format, e, i)
                }));
                match played {
                    Ok(Ok(m)) => res.push(m.result),
                    Ok(Err(err)) => panic!("event {e} match {i}: {err}"),
                    Err(p) => {
                        let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                        match_panics.push(format!(
                            "event {e} match {i} (seed {e}): {}\n    spec: {}",
                            msg.chars().take(160).collect::<String>(),
                            draft_bots::match_spec(setup, max_turns, format)
                        ));
                        res.push([0.5, 0.5]);
                    }
                }
            }
            heuristic_matches += res.len() as u64;
            g.resolve(&res).unwrap();
        } else {
            let res: Vec<[f32; 2]> = (0..n).map(|_| [[1.0, 0.0], [0.0, 1.0], [0.5, 0.5]][rng.below(3) as usize]).collect();
            g.resolve(&res).unwrap();
        }
        let r = g.result();
        assert!((r.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        team_wins[match g.outcome().unwrap().team[0] {
            x if x == 1.0 => 0,
            x if x == 0.0 => 1,
            _ => 2,
        }] += 1;
    }
    println!(
        "{events} events, {steps} actions, searched picks {} deckbuilding {}, {worlds} worlds, {options_done} options completed, {heuristic_matches} heuristic-played matches, \
         team A wins / losses / draws {team_wins:?}, {:.1} s",
        searched[0],
        searched[1],
        t0.elapsed().as_secs_f64()
    );
    for p in &match_panics {
        println!("MTG ENGINE PANIC in a heuristic-played match: {p}");
    }
    for (k, what) in ["minimum 22 (the default)", "minimum 0, 40 or random"].iter().enumerate() {
        let [n, cards, choices, done] = decks[k];
        println!(
            "random decks, {what}: {n}, {:.2} pool cards each, {:.2} card choices per seat ({:.2} with DONE offered)",
            cards as f64 / n.max(1) as f64,
            choices as f64 / n.max(1) as f64,
            done as f64 / n.max(1) as f64
        );
    }
    assert!(match_panics.is_empty(), "{} heuristic-played matches panicked in the mtg engine (above)", match_panics.len());
}

/// Every value of a draft encoding is finite and within the bound (ENGINE.md
/// 46).
fn assert_bounded(x: &[f32], e: u64, i: u64, viewer: u8) {
    let bound = engine::az::encode::FEATURE_BOUND;
    if let Some((k, v)) = x.iter().enumerate().find(|(_, v)| !(v.is_finite() && v.abs() <= bound)) {
        panic!("event {e} step {i}: viewer {viewer}'s feature {k} is {v} (bound {bound})");
    }
}

/// Measurements (ENGINE.md 41): `cargo test --release -p engine --test draft
/// measure -- --ignored --nocapture`.
#[test]
#[ignore]
fn measure() {
    let n: u64 = std::env::var("DRAFT_EVENTS").ok().and_then(|s| s.parse().ok()).unwrap_or(2000);
    for players in [8usize, 4] {
        let mut rng = Rng::new(1);
        let mut keys = Vec::new();
        let (mut decisions, mut searched_picks, mut searched_build, mut build_actions) = (0u64, 0u64, 0u64, 0u64);
        let mut pool_cards = Vec::new();
        let t0 = std::time::Instant::now();
        for e in 0..n {
            let mut g = new(players, e);
            while g.to_move().is_some() {
                let l = legal(&g);
                if l.len() >= 2 {
                    if g.phase() == Phase::Draft {
                        searched_picks += 1;
                    } else {
                        searched_build += 1;
                    }
                }
                build_actions += (g.phase() == Phase::Build) as u64;
                keys.clear();
                g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
                decisions += 1;
            }
            pool_cards.extend((0..players as u8).map(|s| g.build(s).unwrap().pool_cards() as u64));
        }
        let secs = t0.elapsed().as_secs_f64();
        let seats = (n * players as u64) as f64;
        println!(
            "{players} seats: {n} random events in {secs:.2} s = {:.0} events/s ({:.3} us per action); per event {:.1} actions, \
             {:.1} searched picks, {:.1} searched deckbuilding decisions; per seat {:.2} deckbuilding decisions ({:.2} searched), \
             {:.2} pool cards in its deck ({}..={})",
            n as f64 / secs,
            secs * 1e6 / decisions as f64,
            decisions as f64 / n as f64,
            searched_picks as f64 / n as f64,
            searched_build as f64 / n as f64,
            build_actions as f64 / seats,
            searched_build as f64 / seats,
            pool_cards.iter().sum::<u64>() as f64 / seats,
            pool_cards.iter().min().unwrap(),
            pool_cards.iter().max().unwrap()
        );
        // `consensus` decks: their pool cards and deckbuilding decisions.
        let (mut cards, mut acts, mut searched) = (0u64, 0u64, 0u64);
        let m = 50u64;
        for e in 0..m {
            let mut g = new(players, 900 + e);
            let mut bot = Consensus::default();
            while g.to_move().is_some() {
                let l = legal(&g);
                let a = if l.len() == 1 { l[0] } else { bot.choose(&g, &l) };
                if g.phase() == Phase::Build {
                    acts += 1;
                    searched += (l.len() >= 2) as u64;
                }
                g.step(a, &mut keys).unwrap();
            }
            cards += (0..players as u8).map(|s| g.build(s).unwrap().pool_cards() as u64).sum::<u64>();
        }
        let seats = (m * players as u64) as f64;
        println!(
            "{players} seats: consensus decks {:.2} pool cards, {:.2} deckbuilding decisions per seat ({:.2} with two options or more)",
            cards as f64 / seats,
            acts as f64 / seats,
            searched as f64 / seats
        );
        // encode, clone + determinize on sampled states
        let st = states(players, 40, 10, 3);
        let mut x = vec![0.0f32; NUM_FEATURES];
        let reps = 20;
        let t0 = std::time::Instant::now();
        for _ in 0..reps {
            for g in &st {
                x.fill(0.0);
                g.encode(g.to_move().unwrap(), &mut x);
            }
        }
        let enc_us = t0.elapsed().as_secs_f64() * 1e6 / (reps * st.len()) as f64;
        let mut zrng = zr(1);
        let mut sink = 0usize;
        let mut det = [0.0f64; 2];
        let mut det_n = [0usize; 2];
        for phase in 0..2 {
            let sub: Vec<&TeamDraftAz> = st.iter().filter(|g| (g.phase() != Phase::Draft) as usize == phase).collect();
            let t0 = std::time::Instant::now();
            for _ in 0..reps {
                for g in &sub {
                    let mut w = (*g).clone();
                    w.determinize(g.to_move().unwrap(), &mut zrng);
                    sink += w.rest().len();
                }
            }
            det[phase] = t0.elapsed().as_secs_f64() * 1e6 / (reps * sub.len().max(1)) as f64;
            det_n[phase] = sub.len();
        }
        let det_us = format!("draft {:.2} us ({} states), deckbuilding {:.2} us ({} states)", det[0], det_n[0], det[1], det_n[1]);
        let t0 = std::time::Instant::now();
        for _ in 0..reps {
            for g in &st {
                let w = g.clone();
                sink += w.rest().len();
            }
        }
        let clone_us = t0.elapsed().as_secs_f64() * 1e6 / (reps * st.len()) as f64;
        println!(
            "{players} seats: encode {enc_us:.2} us (zeroing excluded: {} floats), clone {clone_us:.2} us, clone + determinize: {det_us} ({} states) [{sink}]",
            NUM_FEATURES,
            st.len()
        );
        // heuristic-played matches
        let m = 10u64;
        let t0 = std::time::Instant::now();
        let mut games = 0u64;
        for e in 0..m {
            let mut g = new(players, 500 + e);
            finish_random(&mut g, &mut rng);
            games += draft_bots::resolve_with_heuristic(&mut g, e).unwrap().iter().map(|r| r.games as u64).sum::<u64>();
        }
        println!(
            "{players} seats: heuristic-played matches {:.3} s per event (random drafts; {} games per event)",
            t0.elapsed().as_secs_f64() / m as f64,
            games as f64 / m as f64
        );
    }
}
