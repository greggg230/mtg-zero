//! M8b part a (ENGINE.md 36): the best-of-three match as the `mtg` episode
//! (CR 103.1: who chooses in later games; the match's end and result),
//! sideboarding between games (CR 100.2b, 100.4b: legal decks, every deck
//! reachable once), the pools and the deckbuilder, the encoding's match
//! additions, and the best-of-one mode unchanged.

use engine::az::decks::{self, DeckGen};
use engine::az::matches::{MatchPool, Sideboarding, MAX_COUNT};
use engine::az::{encode, pools, Format, MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::act;
use engine::rng::Rng;
use engine::{DecisionKind, Game, GameSpec};
use std::collections::BTreeSet;
use zero_search::game::SearchGame;

fn db() -> &'static CardDb {
    CardDb::cube()
}

fn spec(json: &str) -> MtgSpec {
    MtgSpec::parse(json).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn enc(g: &MtgAz, viewer: usize) -> Vec<f32> {
    let mut x = vec![0.0; encode::NUM_FEATURES];
    g.encode(viewer, &mut x);
    x
}

/// Play a match with random actions (`rng`), or the heuristic in the
/// seats of `bots`; `on_step` sees every state before its action.
fn play(g: &mut MtgAz, rng: &mut Rng, bots: [bool; 2], mut on_step: impl FnMut(&MtgAz)) {
    let mut h = [MtgAz::make_bot(&MtgSpec::default(), "heuristic", 1).unwrap(), MtgAz::make_bot(&MtgSpec::default(), "heuristic", 2).unwrap()];
    let mut keys = Vec::new();
    let mut n = 0;
    while let Some(p) = g.to_move() {
        on_step(g);
        let l = legal(g);
        assert!(!l.is_empty() && l.windows(2).all(|w| w[0] < w[1]));
        let a = if bots[p] { h[p].choose(g, &l) } else { l[rng.below(l.len() as u32) as usize] };
        assert!(l.contains(&a));
        g.step(a, &mut keys).unwrap();
        n += 1;
        assert!(n < 200_000, "the match does not end");
    }
}

#[test]
fn bo1_is_the_game_of_before() {
    // The default spec is one game: the same game as `Game::with_sideboards`
    // with the spec's decks and seed, action for action (legal actions,
    // keys, result), and the episode ends with it.
    let sp = MtgSpec::default();
    assert_eq!(sp.format, Format::Bo1);
    assert!(!sp.pools);
    for seed in 0..40u64 {
        let mut m = MtgAz::new_game(&sp, 2, seed).unwrap();
        let [(d0, s0), (d1, s1)] = sp.pools(seed);
        let mut g = Game::with_sideboards(db(), GameSpec::default(), [&d0, &d1], [&s0, &s1], seed).unwrap();
        let mut rng = Rng::new(seed);
        let (mut k1, mut k2, mut l2) = (Vec::new(), Vec::new(), Vec::new());
        while let Some(p) = m.to_move() {
            assert_eq!(Some(p as u8), g.to_move());
            let l = legal(&m);
            g.legal_actions(&mut l2);
            assert_eq!(l, l2);
            let a = l[rng.below(l.len() as u32) as usize];
            k1.clear();
            k2.clear();
            m.step(a, &mut k1).unwrap();
            g.apply(a, &mut k2).unwrap();
            assert_eq!(k1, k2);
        }
        assert!(g.is_over() && m.sideboarding().is_none());
        assert_eq!(&m.result()[..2], &g.result_values()[..]);
        assert_eq!(m.rounds(), g.turn().number);
    }
}

#[test]
fn spec_format_and_pools_keys() {
    let s = spec(r#"{"format": "bo3"}"#);
    assert_eq!((s.format, s.pools), (Format::Bo3, true));
    let s = spec(r#"{"format": "bo3", "pools": false}"#);
    assert_eq!((s.format, s.pools), (Format::Bo3, false));
    let s = spec(r#"{"pools": true}"#);
    assert_eq!((s.format, s.pools), (Format::Bo1, true));
    assert!(MtgSpec::parse(r#"{"format": "bo5"}"#).is_err());
    assert!(MtgSpec::parse(r#"{"pools": 1}"#).is_err());
    // The evaluation spec keeps the format: evaluation and Elo games are
    // matches too.
    let e = spec(&MtgSpec::eval_json(r#"{"format": "bo3"}"#).unwrap());
    assert_eq!((e.format, e.pools), (Format::Bo3, true));
}

#[test]
fn cr_103_1_the_loser_chooses_and_after_a_draw_the_previous_chooser() {
    let sp = spec(r#"{"format": "bo3", "max_turns": 30}"#);
    let mut seen = [0u32; 2];
    for seed in 0..60u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        let mut rng = Rng::new(seed ^ 77);
        let mut game_no = 1;
        let mut checked = 0;
        play(&mut g, &mut rng, [seed % 3 == 0, seed % 2 == 0], |s| {
            if s.m.game_no != game_no && s.sideboarding().is_none() {
                // The first decision of a later game: the chooser's.
                game_no = s.m.game_no;
                let prev = s.m.results[(game_no - 2) as usize];
                let want = match prev {
                    3 => s.m.choosers[(game_no - 2) as usize],
                    w => 1 - (w - 1),
                };
                assert_eq!(s.g.turn().chooser, want, "seed {seed} game {game_no}: results {:?}", s.m.results);
                assert_eq!(s.g.pending_decision().unwrap().kind, DecisionKind::PlayDraw);
                assert_eq!(s.to_move(), Some(want as usize));
                seen[(prev == 3) as usize] += 1;
                checked += 1;
            }
        });
        assert!(g.match_over());
        assert_eq!(checked as u8, g.m.game_no - 1);
    }
    assert!(seen[0] > 20, "later games after a win: {seen:?}");
}

#[test]
fn drawn_games_count_and_a_match_can_be_drawn() {
    // Two turns: every game is a draw by the turn cap. Three games are
    // played, the chooser stays game 1's (103.1), and the match is a draw.
    let sp = spec(r#"{"format": "bo3", "max_turns": 2}"#);
    for seed in 0..10u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        let mut rng = Rng::new(seed);
        play(&mut g, &mut rng, [false, false], |_| {});
        assert_eq!((g.m.game_no, g.m.results, g.m.draws, g.m.wins), (3, [3, 3, 3], 3, [0, 0]));
        assert!(g.m.choosers.iter().all(|&c| c == g.m.choosers[0]));
        assert_eq!(&g.result()[..2], &[0.5, 0.5]);
        // Equal wins: the margin is the last game's life margin / 4.
        assert!(g.margin(0).abs() <= 0.25 && (g.margin(0) + g.margin(1)).abs() < 1e-6);
    }
}

#[test]
fn a_match_ends_at_two_wins_or_after_three_games() {
    let sp = spec(r#"{"format": "bo3"}"#);
    let mut lengths = [0u32; 4];
    for seed in 0..60u64 {
        let mut g = MtgAz::new_game(&sp, 2, 1000 + seed).unwrap();
        let mut rng = Rng::new(seed);
        let mut games_started = 1;
        let mut last_game = 1;
        play(&mut g, &mut rng, [seed % 2 == 0, seed % 3 != 0], |s| {
            assert!(!s.match_over());
            if s.m.game_no != last_game {
                last_game = s.m.game_no;
                games_started += 1;
            }
            // While the match runs nobody has two wins, and a later game
            // follows a finished one.
            assert!(s.m.wins.iter().all(|&w| w < 2));
        });
        let m = &g.m;
        let n = m.game_no as usize;
        assert_eq!(games_started, n);
        assert!(m.results[..n].iter().all(|&r| r != 0) && m.results[n..].iter().all(|&r| r == 0));
        assert_eq!(m.wins[0] + m.wins[1] + m.draws, m.game_no);
        assert!(m.wins.contains(&2) || n == 3);
        assert!(!(n == 3 && m.results[..2].iter().filter(|&&r| r == 1).count() == 2));
        let r = g.result();
        let want = match m.wins[0].cmp(&m.wins[1]) {
            std::cmp::Ordering::Greater => [1.0, 0.0],
            std::cmp::Ordering::Less => [0.0, 1.0],
            std::cmp::Ordering::Equal => [0.5, 0.5],
        };
        assert_eq!(&r[..2], &want);
        assert!((-1.0..=1.0).contains(&g.margin(0)) && (g.margin(0) + g.margin(1)).abs() < 1e-6);
        lengths[n] += 1;
    }
    assert!(lengths[2] > 0 && lengths[3] > 0, "{lengths:?}");
}

/// Every count vector a DFS over the sideboarding decisions reaches, with
/// the number of paths to each.
fn reachable(pool: &MatchPool, start: Vec<u8>) -> (Vec<Vec<u8>>, usize, usize) {
    let mut out = Vec::new();
    let mut nodes = 0;
    let mut deepest = 0;
    let mut stack = vec![(Sideboarding::new(0, start, pool), 0usize)];
    while let Some((sb, depth)) = stack.pop() {
        nodes += 1;
        deepest = deepest.max(depth);
        assert!(!sb.decision.options.is_empty(), "a dead end: {:?}", sb.counts);
        for o in &sb.decision.options {
            let mut c = sb.clone();
            if c.choose(pool, o.action).unwrap() {
                out.push(c.counts.clone());
            } else {
                stack.push((c, depth + 1));
            }
        }
    }
    (out, nodes, deepest)
}

#[test]
fn cr_100_2b_sideboarding_reaches_every_legal_deck_exactly_once() {
    // A small list (two single cards, a card with two copies, two basics)
    // with decks of 5 to 7 cards: the DFS over the decisions reaches every
    // count vector within the copies with 5..=7 cards, each by one path.
    let pool = MatchPool {
        list: vec![(10, 1), (11, 1), (12, 2), (20, MAX_COUNT), (21, MAX_COUNT)],
        n_cards: 3,
        first_picks: vec![],
        min_deck: 5,
        max_deck: 7,
    };
    for start in [vec![1, 0, 1, 2, 2], vec![0, 0, 0, 3, 3], vec![1, 1, 2, 3, 0]] {
        let (got, nodes, deepest) = reachable(&pool, start.clone());
        let set: BTreeSet<Vec<u8>> = got.iter().cloned().collect();
        assert_eq!(set.len(), got.len(), "a deck reached twice");
        let mut want = BTreeSet::new();
        for a in 0..=1u8 {
            for b in 0..=1u8 {
                for c in 0..=2u8 {
                    for p in 0..=7u8 {
                        for i in 0..=7u8 {
                            let n = (a + b + c + p + i) as usize;
                            if (5..=7).contains(&n) {
                                want.insert(vec![a, b, c, p, i]);
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(set, want, "start {start:?}");
        // Keeping the deck is one decision; changing every entry at most
        // 1 + singles + 2 x the others.
        assert!(deepest <= 1 + 2 + 2 * 3, "{deepest}");
        println!("start {start:?}: {} decks, {nodes} decision nodes, depth {deepest}", got.len());
    }
}

#[test]
fn sideboarded_decks_are_legal_and_played_in_the_next_game() {
    // Random sideboarding in random matches: every deck a player ends with
    // is 40..=60 cards of their pool and basics, and the next game's
    // libraries and sideboards are exactly the chosen decks and the rest
    // of the pools.
    let sp = spec(r#"{"format": "bo3", "max_turns": 6}"#);
    let mut changed = 0;
    for seed in 0..40u64 {
        let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
        let mut rng = Rng::new(seed * 31);
        let mut keys = Vec::new();
        let pools = g.m.pools.clone();
        let mut before = [g.m.decks[0].clone(), g.m.decks[1].clone()];
        while g.to_move().is_some() {
            let l = legal(&g);
            let was = g.sideboarding().map(|s| s.decision.kind);
            let a = l[rng.below(l.len() as u32) as usize];
            g.step(a, &mut keys).unwrap();
            if was.is_some() && g.sideboarding().is_none() {
                // A new game: its decks.
                for p in 0..2u8 {
                    let deck = g.m.deck(p);
                    let pool = &pools[p as usize];
                    assert!((40..=60).contains(&deck.len()), "{} cards", deck.len());
                    let mut lib: Vec<u16> = g.g.player(p).library.iter().map(|&c| g.g.obj(c).def).collect();
                    lib.extend(g.g.player(p).hand.iter().map(|&c| g.g.obj(c).def));
                    lib.sort_unstable();
                    assert_eq!(lib, deck, "the game's cards are the deck");
                    let mut side = pool.sideboard(&g.m.decks[p as usize]);
                    side.sort_unstable();
                    assert_eq!(&g.g.player(p).sideboard[..], &side[..]);
                    for (k, &(d, n)) in pool.list.iter().enumerate() {
                        let c = deck.iter().filter(|&&x| x == d).count();
                        assert_eq!(c, g.m.decks[p as usize][k] as usize);
                        assert!(c <= n as usize);
                    }
                    changed += (g.m.decks[p as usize] != before[p as usize]) as u32;
                }
                before = [g.m.decks[0].clone(), g.m.decks[1].clone()];
            }
        }
    }
    assert!(changed > 0);
}

#[test]
fn the_heuristic_keeps_its_deck_and_matches_end() {
    let sp = spec(r#"{"format": "bo3"}"#);
    for seed in 0..8u64 {
        let mut g = MtgAz::new_game(&sp, 2, 50 + seed).unwrap();
        let first = [g.m.decks[0].clone(), g.m.decks[1].clone()];
        let mut rng = Rng::new(seed);
        let mut sb_decisions = 0;
        play(&mut g, &mut rng, [true, true], |s| sb_decisions += s.sideboarding().is_some() as u32);
        assert!(g.match_over());
        assert_eq!([g.m.decks[0].clone(), g.m.decks[1].clone()], first);
        // One decision per player per sideboarding: `PASS`.
        assert_eq!(sb_decisions, 2 * (g.m.game_no as u32 - 1));
    }
}

#[test]
fn sideboarding_is_hidden_from_the_other_player() {
    // During sideboarding the other player's encoding does not show the
    // deck being built (nor, for the second player, the first player's
    // chosen deck): a simultaneous choice made sequential. The decider's
    // shows its pool list, its options and its deck.
    let sp = spec(r#"{"format": "bo3", "max_turns": 2}"#);
    let mut g = MtgAz::new_game(&sp, 2, 3).unwrap();
    let mut keys = Vec::new();
    let mut rng = Rng::new(9);
    while g.sideboarding().is_none() {
        let l = legal(&g);
        g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
    }
    let first = g.to_move().unwrap();
    let other = 1 - first;
    let x0 = enc(&g, other);
    let me0 = enc(&g, first);
    // The decider's list: its pool by definition, then the basics.
    let pool = g.m.pools[first].clone();
    let present = |x: &[f32]| (0..encode::LIST).filter(|&k| x[encode::O_LIST + k * encode::LIST_W + encode::CODE] > 0.5).count();
    assert_eq!(present(&me0), pool.list.len());
    assert_eq!(present(&x0), 0);
    assert_eq!(me0[encode::O_CONTEXT + DecisionKind::Sideboard as usize], 1.0);
    // Change two cards in or out (single entries), then pass.
    let singles: Vec<u16> = legal(&g).into_iter().filter(|&a| a >= act::LIBRARY && ((a - act::LIBRARY) as usize) < pool.n_cards).take(2).collect();
    let mut h = g.clone();
    for &a in &singles {
        if legal(&h).contains(&a) {
            h.step(a, &mut keys).unwrap();
        }
    }
    assert_eq!(enc(&h, other), x0, "the deck being built is not shown to the other player");
    assert_ne!(enc(&h, first), me0);
    // Both versions finish; the second player's view is the same.
    let mut pass = |s: &mut MtgAz| {
        while s.to_move() == Some(first) {
            let l = legal(s);
            let a = if l.contains(&act::PASS) { act::PASS } else { l[0] };
            s.step(a, &mut keys).unwrap();
        }
    };
    pass(&mut g);
    pass(&mut h);
    assert_ne!(g.sideboarding().unwrap().done[first], h.sideboarding().unwrap().done[first]);
    assert_eq!(enc(&g, other), enc(&h, other), "the first player's deck is not shown to the second");
    // determinize keeps the decider's options and encoding (both viewers).
    for v in 0..2 {
        let mut d = g.clone();
        d.determinize(v, &mut zero_search::rng::Rng::new(5));
        assert_eq!(legal(&d), legal(&g));
        assert_eq!(enc(&d, v), enc(&g, v));
    }
}

#[test]
fn determinize_keeps_the_view_in_matches() {
    let sp = spec(r#"{"format": "bo3", "max_turns": 16}"#);
    for seed in 0..12u64 {
        let mut g = MtgAz::new_game(&sp, 2, 400 + seed).unwrap();
        let mut rng = Rng::new(seed);
        let mut n = 0u32;
        play(&mut g, &mut rng, [seed % 2 == 0, false], |s| {
            n += 1;
            if n % 17 == 0 {
                let p = s.to_move().unwrap();
                let mut d = s.clone();
                d.determinize(p, &mut zero_search::rng::Rng::new(n as u64));
                assert_eq!(legal(&d), legal(s));
                assert_eq!(enc(&d, p), enc(s, p));
            }
        });
    }
}

#[test]
fn match_encoding_scores_wins_and_revealed_cards() {
    let sp = spec(r#"{"format": "bo3", "max_turns": 20}"#);
    let mut revealed = 0;
    for seed in 0..10u64 {
        let mut g = MtgAz::new_game(&sp, 2, 900 + seed).unwrap();
        let mut rng = Rng::new(seed);
        play(&mut g, &mut rng, [true, true], |s| {
            let p = s.to_move().unwrap();
            let x = enc(s, p);
            let mx = &x[encode::O_MATCH..encode::O_MATCH + encode::MATCH];
            assert_eq!(mx[encode::mtch::BO3], 1.0);
            assert_eq!(mx[encode::mtch::GAME], s.m.game_no as f32 / 3.0);
            assert_eq!(mx[encode::mtch::MY_WINS], s.m.wins[p] as f32 / 2.0);
            assert_eq!(mx[encode::mtch::OPP_WINS], s.m.wins[1 - p] as f32 / 2.0);
            // The opponent's pool bag: their whole pool (open decklists).
            let bag = &x[encode::O_BAGS + encode::bag::OPP_POOL * encode::D..][..encode::D];
            let n: f32 = bag.iter().sum();
            assert_eq!(n, 0.25 * s.m.pools[1 - p].list[..s.m.pools[1 - p].n_cards].iter().map(|e| e.1 as f32).sum::<f32>());
            let rb = &x[encode::O_BAGS + encode::bag::OPP_REVEALED * encode::D..][..encode::D];
            if s.m.game_no == 1 && s.sideboarding().is_none() {
                assert!(rb.iter().all(|&v| v == 0.0));
            } else {
                revealed += rb.iter().filter(|&&v| v > 0.0).count();
            }
        });
    }
    assert!(revealed > 0);
}

#[test]
fn pools_and_the_deckbuilder() {
    let gen = DeckGen::default();
    let basics = pools::basic_ids(db());
    let comps = decks::companions(db());
    let (mut with_comp, mut short) = (0, 0);
    for seed in 0..200u64 {
        let mut rng = Rng::new(seed);
        let p = pools::random_pool(db(), &mut rng);
        assert_eq!(p.cards.len(), pools::POOL_SIZE);
        assert!(p.cards.windows(2).all(|w| w[0] < w[1]), "sorted, singleton");
        assert!(p.cards.iter().all(|&c| !basics.contains(&c) && !db().def(c).token && (c as usize) < db().num_cards()));
        assert_eq!(p.first_picks.len(), pools::FIRST_PICKS);
        assert!(p.first_picks.iter().all(|c| p.cards.contains(c)));
        with_comp += p.cards.iter().any(|c| comps.contains(c)) as u32;
        let (deck, side) = pools::build_deck(db(), &p.cards, &gen);
        assert_eq!(deck.len(), 40);
        let spells = deck.iter().filter(|&&c| !db().def(c).is(engine::db::types::LAND)).count();
        if spells == pools::SPELLS {
            decks::check(db(), &deck, &gen).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        } else {
            short += 1;
        }
        // Deck + sideboard = pool + basics.
        let mut all: Vec<u16> = deck.iter().copied().filter(|c| !basics.contains(c)).chain(side.iter().copied()).collect();
        all.sort_unstable();
        assert_eq!(all, p.cards);
        // Seeded.
        assert_eq!(pools::random_pool(db(), &mut Rng::new(seed)), p);
    }
    println!("{with_comp} of 200 pools with a companion, {short} without 23 playable spells");
    assert!(with_comp > 20, "companions in {with_comp} pools");
    assert!(short <= 2, "{short} pools without 23 playable spells");
    // The evaluation pools: fixed per (seed, index), different across indices.
    assert_eq!(pools::eval_pools(db(), 1, 3), pools::eval_pools(db(), 1, 3));
    assert_ne!(pools::eval_pools(db(), 1, 3), pools::eval_pools(db(), 1, 4));
    let e = spec(&MtgSpec::eval_json(r#"{"format": "bo3"}"#).unwrap());
    let seats = e.seats(5);
    assert!(seats.iter().all(|s| s.deck.len() == 40 && s.first_picks.len() == 3));
}
