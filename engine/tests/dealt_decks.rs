//! Dealt decks (ENGINE.md 154, the spec's `"dealt_decks"`): in a share `p` of a constructed best-of-three's
//! matches one seat (each 1/2) has its 60 for each later game dealt from its 75 - uniform over the swap counts of
//! the spec's range, then uniform over the 60s with that count (`ConstructedPrior::dealt`), each game's deal
//! independent - with no sideboarding decision of its own. Public to both players (`encode::g6::DEALT`); the other
//! player's worlds deal the dealt seat's 60s from the same restricted prior, untilted; the deal is keyed private to
//! the dealt seat (`KEY_DEALT`); the aux `deck` target is empty for a viewer whose opponent's 60 was dealt.

use engine::az::bots::Heuristic;
use engine::az::constructed::Decklist;
use engine::az::encode::{g6, mtch, NUM_FEATURES, O_GLOBALS_V6, O_MATCH};
use engine::az::hidden_decks::{check_sample, ConstructedPrior, DeckPrior, DeckTilt, DeckWant};
use engine::az::identity::D;
use engine::az::matches::{dealt_deal_seed, dealt_seat, game_one_winner};
use engine::az::{DealtDecks, MtgAz, MtgSpec, Share};
use engine::db::CardDb;
use engine::decision::act;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DEALT, KEY_GAME};
use engine::rng::{split64, Rng};
use engine::DecisionKind;
use serde_json::json;
use zero_search::driver::{Driver, DriverCfg, GameResult, KeyKind, NnSeat, Record, SeatCfg};
use zero_search::game::{Belief, Next, SearchGame, MAX_SEATS};

fn scale(n: u64) -> u64 {
    n * std::env::var("AZ_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

/// The Delver mirror's constructed best-of-three (both seats the pinned 75, ENGINE.md 120), post-sideboard games
/// only with one episode per game (the presets' match, ENGINE.md 130), and the keys of `extra` (a null value
/// removes the key).
fn json_of(extra: serde_json::Value) -> String {
    let d = Decklist::izzet_delver();
    let main = d.main_names();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    let mut v = json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true, "start_game": 2,
        "decks": {"p0": main, "p1": main, "p0_sideboard": side, "p1_sideboard": side}});
    for (k, x) in extra.as_object().unwrap() {
        if x.is_null() {
            v.as_object_mut().unwrap().remove(k);
        } else {
            v[k] = x.clone();
        }
    }
    v.to_string()
}

fn spec_of(extra: serde_json::Value) -> MtgSpec {
    MtgSpec::parse(&json_of(extra)).unwrap()
}

/// `{"dealt_decks": {"p": p, "swaps": [lo, hi]}}`.
fn dd(p: f64, lo: u8, hi: u8) -> serde_json::Value {
    json!({"dealt_decks": {"p": p, "swaps": [lo, hi]}})
}

/// `a` with the keys of `b` (objects).
fn with(a: serde_json::Value, b: serde_json::Value) -> serde_json::Value {
    let mut a = a;
    for (k, x) in b.as_object().unwrap() {
        a[k] = x.clone();
    }
    a
}

fn db() -> &'static CardDb {
    CardDb::constructed()
}

/// The Delver 75: the registered main and the sideboard (card ids).
fn delver_ids() -> (Vec<u16>, Vec<u16>) {
    let d = Decklist::izzet_delver();
    (Decklist::ids(db(), &d.main).unwrap(), Decklist::ids(db(), &d.sideboard).unwrap())
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// The whole layout v7 row of `viewer` (the head first).
fn row(g: &MtgAz, viewer: usize) -> Vec<f32> {
    engine::az::tail_encode::Row::of_az(g, viewer).flat()
}

/// The next episode after a game's end with one episode per game; None at the match's end (and always for a whole
/// match: one episode).
fn next(g: &MtgAz) -> Option<MtgAz> {
    if !g.m.game_episodes {
        return None;
    }
    match g.next_episode(None).unwrap() {
        Next::Continue(n) => Some(n),
        Next::Over(_) => None,
    }
}

/// Half the heuristic's choice, half a random one (as post_board.rs's).
fn mixed(g: &MtgAz, h: &mut Heuristic, rng: &mut Rng) -> u16 {
    let l = legal(g);
    if rng.below(2) == 0 && g.sideboarding().is_none() {
        h.choose(&g.g, &l)
    } else {
        let pick = l[rng.below(l.len() as u32) as usize];
        if pick == act::YES && g.g.kind() == Some(DecisionKind::Priority) && l.len() > 1 && rng.below(8) != 0 {
            l[0]
        } else {
            pick
        }
    }
}

/// The swaps of `deck` (sorted) against the registered `main` (sorted): its cards beyond the main's counts.
fn swaps(main: &[u16], deck: &[u16]) -> usize {
    let mut m = main.to_vec();
    let mut extra = 0;
    for &d in deck {
        match m.binary_search(&d) {
            Ok(i) => {
                m.remove(i);
            }
            Err(_) => extra += 1,
        }
    }
    extra
}

fn sorted(mut v: Vec<u16>) -> Vec<u16> {
    v.sort_unstable();
    v
}

/// The cards `viewer` has seen of the other player's deck (fuzz.rs's `hd_shown`): every world's deck holds them.
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
    for (i, ob) in g.objs().iter().enumerate() {
        let c = engine::CardIdx::new(i as u32);
        if ob.owner == o && !r.contains(&i) && matches!(ob.zone, engine::Zone::Hand | engine::Zone::Library | engine::Zone::FaceDown) && !g.knows(viewer, c) {
            if let Ok(at) = v.binary_search(&engine::az::sample::card_of(g, c)) {
                v.remove(at);
            }
        }
    }
    v
}

fn holds(deck: &[u16], cards: &[u16]) -> bool {
    let mut left = deck.to_vec();
    cards.iter().all(|x| match left.binary_search(x) {
        Ok(i) => {
            left.remove(i);
            true
        }
        Err(_) => false,
    })
}

/// The 60 a match of seed `seed` deals its dealt seat for game `game` (ENGINE.md 154): the restricted prior's sample
/// from that game's stream of the match's seed - nothing played in the match reaches it.
fn the_deal(seed: u64, game: u8, lo: usize, hi: usize) -> Vec<u16> {
    let (main, side) = delver_ids();
    let prior = ConstructedPrior::dealt(&main, &side, lo, hi);
    let want = DeckWant { db: db(), n: 60, shown: &[], companion: None, tilt: None, pick: None };
    prior.sample(&want, &mut Rng::new(split64(dealt_deal_seed(seed) ^ game as u64))).deck
}

#[test]
fn dealt_decks_is_a_constructed_best_of_three_self_play_setting() {
    let s = spec_of(dd(0.5, 1, 8));
    assert_eq!(s.dealt_decks, Some(DealtDecks { p: Share(0.5), swaps: (1, 8) }));
    assert_eq!(spec_of(json!({})).dealt_decks, None);
    assert_eq!(spec_of(dd(1.0, 0, 15)).dealt_decks.unwrap().swaps, (0, 15));
    assert_eq!(spec_of(dd(0.25, 3, 3)).dealt_decks.unwrap().swaps, (3, 3));
    // whole matches (game 1 played, one episode per match) take it too
    assert!(spec_of(with(dd(0.5, 1, 8), json!({"start_game": null, "game_episodes": null}))).dealt_decks.is_some());
    let err = |extra: serde_json::Value| MtgSpec::parse(&json_of(extra)).unwrap_err();
    for (x, want) in [
        (json!({"p": 0, "swaps": [1, 8]}), "(0, 1]"),
        (json!({"p": 1.5, "swaps": [1, 8]}), "(0, 1]"),
        (json!({"p": "half", "swaps": [1, 8]}), "(0, 1]"),
        (json!({"swaps": [1, 8]}), "(0, 1]"),
        (json!({"p": 0.5}), "[lo, hi]"),
        (json!({"p": 0.5, "swaps": [1]}), "[lo, hi]"),
        (json!({"p": 0.5, "swaps": [8, 1]}), "lo <= hi"),
        (json!({"p": 0.5, "swaps": [-1, 8]}), "lo <= hi"),
        (json!({"p": 0.5, "swaps": [1, 16]}), "<= 15"),
        (json!({"p": 0.5, "swaps": [1, 8], "seat": 0}), "unknown dealt_decks key"),
        (json!([0.5, 1, 8]), "must be"),
    ] {
        let e = err(json!({"dealt_decks": x}));
        assert!(e.contains(want), "{x}: {e}");
    }
    // the range within each seat's own sideboard: P1 with 12 sideboard cards takes at most 12 swaps
    let d = Decklist::izzet_delver();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    let short: Vec<String> = side[..12].to_vec();
    let decks = json!({"p0": d.main_names(), "p1": d.main_names(), "p0_sideboard": side, "p1_sideboard": short});
    assert!(spec_of(with(dd(0.5, 1, 12), json!({"decks": decks}))).dealt_decks.is_some());
    let e = err(with(dd(0.5, 1, 13), json!({"decks": decks})));
    assert!(e.contains("p1's sideboard has 12 cards"), "{e}");
    // refused outside a constructed best-of-three, and with keep_deck (the yardstick's)
    let bo1 = with(dd(0.5, 1, 8), json!({"constructed": true, "decks": {"p0": d.main_names(), "p1": d.main_names()}}));
    assert!(MtgSpec::parse(&bo1.to_string()).unwrap_err().contains("constructed best-of-three"));
    let limited = with(dd(0.5, 1, 8), json!({"format": "bo3"}));
    assert!(MtgSpec::parse(&limited.to_string()).unwrap_err().contains("constructed best-of-three"));
    let e = err(with(dd(0.5, 1, 8), json!({"keep_deck": [0]})));
    assert!(e.contains("keep_deck"), "{e}");
    let e = err(with(dd(0.5, 1, 8), json!({"keep_deck": [1]})));
    assert!(e.contains("keep_deck"), "{e}");
    // the evaluation spec drops it (and keeps the rest)
    let e = MtgSpec::parse(&MtgSpec::eval_json(&json_of(dd(0.5, 1, 8))).unwrap()).unwrap();
    assert_eq!((e.dealt_decks, e.start_game, e.game_episodes), (None, 2, true));
    assert_eq!(MtgSpec::eval_json(&json_of(json!({}))).unwrap(), MtgSpec::eval_json(&json_of(dd(0.5, 1, 8))).unwrap());
}

#[test]
fn the_dealt_prior_is_the_constructed_prior_restricted_to_the_swap_range() {
    let db = db();
    let (main, side) = delver_ids();
    let main_s = sorted(main.clone());
    let (lo, hi) = (1usize, 8usize);
    let prior = ConstructedPrior::dealt(&main, &side, lo, hi);
    let full = ConstructedPrior::new(&main, &side);
    assert_eq!((prior.swap_range(), full.swap_range()), ((1, 8), (0, side.len())));
    assert_eq!(full.name(), "constructed");
    assert!(prior.name().contains("dealt"));
    // its counts: the base prior's in the range, none outside
    for s in 0..=side.len() {
        let want = if (lo..=hi).contains(&s) { full.decks_by_swaps()[s] } else { 0.0 };
        assert_eq!(prior.decks_by_swaps()[s], want, "swaps {s}");
    }
    let plain = DeckWant { db, n: 60, shown: &[], companion: None, tilt: None, pick: None };
    let n = scale(4000) as usize;
    let mut hist = vec![0usize; side.len() + 1];
    let mut mean = vec![0.0f64; db.len()];
    let mut rng = Rng::new(154);
    for _ in 0..n {
        let s = prior.sample(&plain, &mut rng);
        // a legal 60 of the 75 (`check_own`: the deck and the sideboard are the 75)
        check_sample(&prior, &plain, &s).unwrap();
        assert_eq!((s.deck.len(), s.sideboard.len()), (60, 15));
        let k = prior.swaps_of(&s.deck);
        assert_eq!(k, swaps(&main_s, &s.deck));
        hist[k] += 1;
        for &d in &s.deck {
            mean[d as usize] += 1.0 / n as f64;
        }
    }
    // uniform over the swap counts 1..=8: n / 8 each (within 5 sd), none outside
    let e = n as f64 / (hi - lo + 1) as f64;
    let sd = (e * (1.0 - 1.0 / (hi - lo + 1) as f64)).sqrt();
    for (s, &h) in hist.iter().enumerate() {
        if (lo..=hi).contains(&s) {
            assert!((h as f64 - e).abs() < 5.0 * sd, "swaps {s}: {h} of {n} ({hist:?})");
        } else {
            assert_eq!(h, 0, "swaps {s} outside the range ({hist:?})");
        }
    }
    let mean_swaps = hist.iter().enumerate().map(|(s, &h)| (s * h) as f64).sum::<f64>() / n as f64;
    assert!((mean_swaps - 4.5).abs() < 0.15, "mean swaps {mean_swaps}");
    println!("swaps of {n} dealt 60s: {hist:?} (mean {mean_swaps:.2})");
    // the per-card distribution: each card's mean copies is the restricted prior's exact marginal (its expected fill
    // with nothing shown), within 5 standard errors (a card's copies vary by at most its copies in the 75)
    let fill = prior.expected_fill(&plain);
    let full_fill = full.expected_fill(&plain);
    let (pool, _) = engine::az::matches::MatchPool::constructed(&main, &side);
    let mut apart = 0;
    for &(d, t) in &pool.list {
        let tol = 5.0 * (t as f64 / 2.0) / (n as f64).sqrt() + 1e-9;
        assert!((mean[d as usize] - fill[d as usize]).abs() < tol, "{}: mean {} against the marginal {}", db.def(d).name, mean[d as usize], fill[d as usize]);
        apart += ((fill[d as usize] - full_fill[d as usize]).abs() > 0.05) as usize;
    }
    assert!(apart > 5, "the restricted prior's marginals differ from the full prior's ({apart} cards)");
    // conditioned exactly as the full prior (the other player's worlds): shown cards held, the swaps still in range -
    // two Pyroblasts and a Daze shown are two swaps in at least
    let id = |s: &str| db.expect_id(s).unwrap();
    let r = sorted(vec![id("Pyroblast"), id("Pyroblast"), id("Daze")]);
    let want = DeckWant { db, n: 60, shown: &r, companion: None, tilt: None, pick: None };
    let mut hist2 = vec![0usize; side.len() + 1];
    for _ in 0..600 {
        let s = prior.sample(&want, &mut rng);
        check_sample(&prior, &want, &s).unwrap();
        assert!(holds(&s.deck, &r));
        hist2[prior.swaps_of(&s.deck)] += 1;
    }
    assert!(hist2[..2].iter().chain(&hist2[9..]).all(|&h| h == 0), "{hist2:?}");
    // a range past the sideboard, or empty, is refused loudly
    assert!(std::panic::catch_unwind(|| ConstructedPrior::dealt(&main, &side, 1, 16)).is_err());
    assert!(std::panic::catch_unwind(|| ConstructedPrior::dealt(&main, &side, 5, 4)).is_err());
}

#[test]
fn the_dealt_seat_comes_from_the_match_seed() {
    // the share of matches with a dealt seat is p, each seat 1/2 (within 5 sd), from the seed alone
    for p in [0.5, 0.3] {
        let n = scale(8000);
        let (mut dealt, mut seat0) = (0u64, 0u64);
        for seed in 0..n {
            if let Some(d) = dealt_seat(seed, p) {
                dealt += 1;
                seat0 += (d == 0) as u64;
            }
        }
        let sd = (n as f64 * p * (1.0 - p)).sqrt();
        assert!((dealt as f64 - n as f64 * p).abs() < 5.0 * sd, "p {p}: {dealt} of {n} matches dealt");
        let sd0 = (dealt as f64 * 0.25).sqrt();
        assert!((seat0 as f64 - dealt as f64 / 2.0).abs() < 5.0 * sd0, "p {p}: seat 0 in {seat0} of {dealt}");
        println!("p {p}: {dealt} of {n} matches dealt, seat 0 in {seat0}");
    }
    assert!((0..1000).all(|seed| dealt_seat(seed, 1.0).is_some()));
    // the match's own: MtgAz takes it from its seed, deterministically; no seat without the option
    let s = spec_of(dd(0.5, 1, 8));
    let plain = spec_of(json!({}));
    for seed in 0..scale(200) {
        let a = MtgAz::new(&s, seed).unwrap();
        assert_eq!(a.m.dealt, dealt_seat(seed, 0.5), "seed {seed}");
        let b = MtgAz::new(&s, seed).unwrap();
        assert_eq!((b.m.dealt, b.m.dealt_seed, legal(&b)), (a.m.dealt, a.m.dealt_seed, legal(&a)));
        assert_eq!(a.sideboarding().unwrap().done, b.sideboarding().unwrap().done);
        assert_eq!(MtgAz::new(&plain, seed).unwrap().m.dealt, None);
    }
}

/// The match of a seed with dealt decks is the real match of that seed but the dealt seat's decks: the same game-1
/// result, the same first sideboarder and chooser, the match's RNG - its games' seeds - unchanged at every game's
/// start (the other seat keeps its main in both, the dealt seat keeps it in the match without the option; game 2
/// conceded by game 1's winner, so a game 3).
#[test]
fn a_dealt_match_is_the_real_match_of_its_seed_but_the_dealt_decks() {
    let s = spec_of(dd(1.0, 1, 8));
    let plain = spec_of(json!({}));
    let mut games = 0;
    for seed in 0..scale(40) {
        let (mut a, mut b) = (MtgAz::new(&s, seed).unwrap(), MtgAz::new(&plain, seed).unwrap());
        let d = a.m.dealt.unwrap();
        assert_eq!((a.m.results, a.m.game_no), (b.m.results, b.m.game_no));
        assert_eq!(a.m.results[0], 1 + game_one_winner(seed));
        assert_eq!(a.m.rng, b.m.rng, "seed {seed}: the match's RNG");
        for game in [2u8, 3] {
            // the sideboarding: every non-dealt decision PASSes (the main kept) in both
            let mut keys = Vec::new();
            while a.sideboarding().is_some() {
                assert_ne!(a.to_move(), Some(d as usize), "seed {seed}: the dealt seat decides its sideboarding");
                a.step(act::PASS, &mut keys).unwrap();
            }
            while b.sideboarding().is_some() {
                b.step(act::PASS, &mut keys).unwrap();
            }
            assert_eq!((a.m.game_no, b.m.game_no), (game, game));
            assert_eq!(a.m.rng, b.m.rng, "seed {seed} game {game}: the match's RNG");
            assert_eq!(a.g.turn().chooser, b.g.turn().chooser, "seed {seed} game {game}: the chooser");
            assert_eq!(a.g.kind(), Some(DecisionKind::PlayDraw));
            assert_eq!(a.m.decks[1 - d as usize], b.m.decks[1 - d as usize], "the other seat's main");
            assert_eq!(a.m.deck(d), the_deal(seed, game, 1, 8), "seed {seed} game {game}: the dealt 60");
            games += 1;
            if game == 3 {
                break;
            }
            // game 2 conceded by game 1's winner: 1-1, a game 3
            let w = game_one_winner(seed) as usize;
            let next_of = |x: &MtgAz| match x.next_episode(Some(w)).unwrap() {
                Next::Continue(n) => n,
                Next::Over(_) => panic!("seed {seed}: 1-1 after game 2"),
            };
            a = next_of(&a);
            b = next_of(&b);
        }
    }
    assert_eq!(games, 2 * scale(40));
}

/// The dealt seat never decides at a sideboarding; the order stays CR 103.1's (the previous game's loser sideboards
/// first - a dealt loser is dealt first - and chooses play or draw, a dealt chooser by its own decision); the other
/// seat sideboards with its full options; the dealt 60 of each later game is that game's deal of the match's seed
/// (independent of everything played: game 3's never conditioned on game 2), with swaps in the range. Post-sideboard
/// matches with one episode per game, and whole matches from game 1 (the deal keyed within the step that ends game 1).
#[test]
fn the_dealt_seat_has_no_sideboarding_decision_and_the_order_is_cr_103_1s() {
    let (main, _) = delver_ids();
    let main_s = sorted(main);
    let (mut matches, mut dealt_first, mut dealt_second, mut dealt_chose, mut threes, mut other_decisions) = (0, 0, 0, 0, 0, 0);
    for (k, extra) in [dd(1.0, 1, 8), with(dd(1.0, 2, 5), json!({"start_game": null, "game_episodes": null, "max_turns": 16}))].into_iter().enumerate() {
        let (lo, hi) = if k == 0 { (1, 8) } else { (2, 5) };
        let s = spec_of(extra);
        for seed in 0..scale(30) {
            let mut g = MtgAz::new(&s, seed).unwrap();
            let d = g.m.dealt.unwrap();
            let (mut rng, mut h, mut keys) = (Rng::new(seed ^ 0xDEA1), Heuristic::new(seed), Vec::new());
            let mut game_seen = 1u8;
            loop {
                let Some(p) = g.to_move() else {
                    match next(&g) {
                        Some(x) => {
                            g = x;
                            continue;
                        }
                        None => break,
                    }
                };
                if let Some(sb) = g.sideboarding() {
                    assert_ne!(p, d as usize, "seed {seed}: the dealt seat decides at a sideboarding");
                    other_decisions += 1;
                    // the dealt seat first: dealt already, its 60 done; the other seat sideboards
                    if sb.order[0] == d && sb.turn == 1 {
                        assert!(sb.done[d as usize].is_some());
                    }
                }
                let a = mixed(&g, &mut h, &mut rng);
                g.step(a, &mut keys).unwrap();
                if g.m.game_no != game_seen && g.sideboarding().is_none() {
                    game_seen = g.m.game_no;
                    let i = (game_seen - 2) as usize;
                    let loser = match g.m.results[i] {
                        3 => g.m.choosers[i],
                        w => 2 - w,
                    };
                    assert_eq!(g.g.turn().chooser, loser, "seed {seed}: game {game_seen}'s chooser (CR 103.1)");
                    assert_eq!(g.g.kind(), Some(DecisionKind::PlayDraw));
                    assert_eq!(g.to_move(), Some(loser as usize));
                    if loser == d {
                        dealt_first += 1;
                        dealt_chose += 1;
                    } else {
                        dealt_second += 1;
                    }
                    let deck = g.m.deck(d);
                    assert_eq!(deck, the_deal(seed, game_seen, lo, hi), "seed {seed} game {game_seen}: the dealt 60 is its game's deal");
                    assert!((lo..=hi).contains(&swaps(&main_s, &deck)));
                    threes += (game_seen == 3) as u64;
                }
            }
            matches += 1;
        }
    }
    println!(
        "{matches} matches: the dealt seat sideboarded first {dealt_first} times (and chose play or draw), second {dealt_second}; \
         {threes} games 3; {other_decisions} sideboarding decisions of the other seat"
    );
    assert!(dealt_first > 0 && dealt_second > 0 && dealt_chose > 0 && threes > 0 && other_decisions > 0);
}

/// Uniform priors and value 0.5 for every row.
fn answer(d: &mut Driver<MtgAz>, b: &zero_search::driver::Batch) {
    let mut pri = Vec::new();
    for r in 0..b.b {
        let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
        pri.extend(std::iter::repeat_n(1.0 / k as f32, k));
    }
    let mut v = vec![0.0; b.b * MAX_SEATS];
    for r in 0..b.b {
        v[r * MAX_SEATS] = 0.5;
        v[r * MAX_SEATS + 1] = 0.5;
    }
    d.submit(b.id, &pri, &v, &vec![0.0; b.b * MAX_SEATS]).unwrap();
}

/// Self-play matches through zero-core's driver (a uniform evaluator, 4 simulations, recording; post_board.rs's):
/// each match's result row and its samples.
fn self_play(json: &str, matches: usize, seed: u64) -> Vec<(GameResult, Vec<Record>)> {
    let mut s = NnSeat::eval(0, 4);
    s.selfplay = true;
    s.temp_moves = 1000;
    let cfg = DriverCfg {
        spec: json.into(),
        seats: vec![SeatCfg::Nn(s)],
        games: vec![vec![0, 0]; matches],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: matches,
        seed,
        record: true,
        threads: 4,
        groups: 2,
        max_steps: 60_000,
        rolling: false,
        net: 0,
        queue_only: false,
        move_log: 0,
        aux: true,
        paired: false,
        sparse: false,
        branch: None,
        starts: None,
        max_row_tokens: None,
    };
    let mut d = Driver::<MtgAz>::new(cfg).unwrap();
    let mut rows = Vec::new();
    let take = |d: &mut Driver<MtgAz>, rows: &mut Vec<(GameResult, Vec<Record>)>| {
        let rs = d.take_results();
        let smp = d.take_samples();
        let mut a = 0;
        for r in rs {
            let k = r.samples as usize;
            rows.push((r, smp[a..a + k].to_vec()));
            a += k;
        }
        assert_eq!(a, smp.len(), "a row's samples follow it");
    };
    while let Some(b) = d.next_batch(true).unwrap() {
        answer(&mut d, &b);
        take(&mut d, &mut rows);
    }
    take(&mut d, &mut rows);
    rows.sort_by_key(|(r, _)| r.gid);
    rows
}

/// `x`'s dealt floats (the frame seat's, then the other's).
fn dealt_floats(x: &[f32]) -> [f32; 2] {
    [x[O_GLOBALS_V6 + g6::DEALT], x[O_GLOBALS_V6 + g6::PLAYER + g6::DEALT]]
}

/// zero-core's self-play driver (recording, the aux targets on) on a dealt spec (every match dealt): no recorded row
/// is a sideboarding decision of a dealt seat (its own dealt float set) - the dealt seat has none -, the other seat's
/// sideboarding rows (the opponent's float set) and the dealt seat's game rows are recorded; every row shows exactly
/// one dealt player; a row whose frame opponent's 60 was dealt has no `deck` target (all zero).
#[test]
fn self_play_records_no_sideboarding_sample_of_the_dealt_seat() {
    let rows = self_play(&json_of(dd(1.0, 1, 8)), 12, 154);
    assert_eq!(rows.len(), 12);
    let (mut sb_other, mut game_dealt, mut game_other, mut n) = (0, 0, 0, 0);
    let (mut deck_rows_vs_dealt, mut deck_rows_vs_other) = (0, 0);
    for (r, smp) in &rows {
        assert!(r.over, "match {}", r.gid);
        for s in smp {
            let x = &s.x;
            let f = dealt_floats(x);
            assert!(f == [1.0, 0.0] || f == [0.0, 1.0], "match {}: dealt floats {f:?}", r.gid);
            let sb = x[O_MATCH + mtch::SIDEBOARDING] > 0.5;
            if sb {
                assert_eq!(f, [0.0, 1.0], "match {}: a sideboarding row of the dealt seat", r.gid);
                sb_other += 1;
            } else if f[0] == 1.0 {
                game_dealt += 1;
            } else {
                game_other += 1;
            }
            // (the aux targets of a game row: belief, then deck - all zero facing a dealt 60; sideboarding rows none)
            if !sb && !s.aux.is_empty() {
                let deck = &s.aux[D..2 * D];
                // (a row framed in the other seat - deciding for a controlled player - has its own frame; the dealt
                // float's frame is the row's, so the float on side 1 is the frame opponent's)
                if f[1] == 1.0 {
                    assert!(deck.iter().all(|&v| v == 0.0), "match {}: a deck target facing a dealt 60", r.gid);
                    deck_rows_vs_dealt += 1;
                } else {
                    deck_rows_vs_other += deck.iter().any(|&v| v != 0.0) as usize;
                }
            }
            n += 1;
        }
    }
    println!(
        "{n} rows: {sb_other} sideboarding rows (all the other seat's), {game_dealt} game rows of the dealt seat, {game_other} of the other; \
         {deck_rows_vs_dealt} rows facing a dealt 60 (no deck target), {deck_rows_vs_other} with a deck target"
    );
    assert!(sb_other > 0 && game_dealt > 0 && game_other > 0 && deck_rows_vs_dealt > 0 && deck_rows_vs_other > 0);
}

/// The encoding: in a dealt match both viewers' rows - in games and at sideboardings - show which player is dealt
/// (`g6::DEALT`, the frame seat first); a match without the option shows none (0, as every existing encoding).
#[test]
fn both_viewers_rows_show_the_dealt_player() {
    let s = spec_of(dd(1.0, 1, 8));
    let plain = spec_of(json!({}));
    let (mut sb_rows, mut game_rows) = (0, 0);
    for seed in 0..scale(16) {
        for (sp, dealt) in [(&s, true), (&plain, false)] {
            let mut g = MtgAz::new(sp, seed).unwrap();
            let (mut rng, mut keys) = (Rng::new(seed ^ 0xE4C), Vec::new());
            for step in 0..400 {
                let Some(_) = g.to_move() else {
                    match next(&g) {
                        Some(x) => {
                            g = x;
                            continue;
                        }
                        None => break,
                    }
                };
                if step % 3 == 0 {
                    for v in 0..2u8 {
                        let x = row(&g, v as usize);
                        assert!(x.len() >= NUM_FEATURES);
                        let frame = if g.sideboarding().is_some() { v } else { g.g.encoding_seat(v) };
                        let want = match g.m.dealt {
                            Some(d) if dealt => [(d == frame) as u8 as f32, (d != frame) as u8 as f32],
                            _ => [0.0, 0.0],
                        };
                        assert_eq!(dealt_floats(&x), want, "seed {seed} step {step}: P{v}'s dealt floats");
                        if dealt {
                            if g.sideboarding().is_some() {
                                sb_rows += 1;
                            } else {
                                game_rows += 1;
                            }
                        }
                    }
                }
                let l = legal(&g);
                g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
            }
        }
    }
    assert!(sb_rows > 0 && game_rows > 0, "{sb_rows} sideboarding rows, {game_rows} game rows");
}

/// The aux targets (zero-core's `aux_target`): in the dealt seat's dealt games the other viewer's `deck` segment is
/// all zero (no target) and its `belief` (the dealt seat's hand) as always; the dealt viewer's own targets as in any
/// match (its opponent chose its 60).
#[test]
fn no_deck_target_facing_a_dealt_60() {
    let s = spec_of(dd(1.0, 1, 8));
    let (mut zeroed, mut kept, mut hands) = (0, 0, 0);
    for seed in 0..scale(16) {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let d = g.m.dealt.unwrap();
        let (mut rng, mut keys) = (Rng::new(seed ^ 0xA0C5), Vec::new());
        for step in 0..600 {
            let Some(_) = g.to_move() else {
                match next(&g) {
                    Some(x) => {
                        g = x;
                        continue;
                    }
                    None => break,
                }
            };
            if g.sideboarding().is_none() && step % 4 == 0 {
                for v in 0..2usize {
                    let mut out = vec![0.0f32; 2 * D];
                    g.aux_target(v, &mut out);
                    let seat = g.g.encoding_seat(v as u8);
                    let (belief, deck) = out.split_at(D);
                    let hand = g.g.player(1 - seat).hand.len();
                    assert_eq!(belief.iter().sum::<f32>() as usize, hand, "seed {seed}: the belief target is the frame opponent's hand");
                    hands += (hand > 0) as usize;
                    if 1 - seat == d {
                        assert!(deck.iter().all(|&x| x == 0.0), "seed {seed} step {step}: a deck target facing a dealt 60");
                        zeroed += 1;
                    } else {
                        assert!(deck.iter().any(|&x| x != 0.0), "seed {seed} step {step}: the dealt viewer's deck target");
                        kept += 1;
                    }
                }
            }
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
    }
    assert!(zeroed > 0 && kept > 0 && hands > 0, "{zeroed} zeroed, {kept} kept");
}

/// The deal's keys: when the dealt seat sideboards second, the other seat's last `PASS` deals it and starts the
/// next game - its keys are the dealt 60 exactly (`KEY_DEALT`, private to the dealt seat: nine 5-bit counts a key),
/// then `KEY_GAME`; in a world of the other seat the same step gives as many `KEY_DEALT` keys, each `HIDDEN`, whatever
/// the world dealt (two worlds' keys equal, their 60s differ), so its tree never branches on the deal; in the dealt
/// seat's worlds the keys are the deal (its tree does). A whole match keys the deal of a dealt first sideboarder in
/// the step that ends the game.
#[test]
fn the_deal_is_keyed_private_to_the_dealt_seat() {
    let s = spec_of(dd(1.0, 1, 8));
    let decode = |keys: &[u64], d: u8| -> Vec<u8> {
        let mut counts = Vec::new();
        for (j, &k) in keys.iter().filter(|&&k| key_kind(k) == KEY_DEALT).enumerate() {
            assert_eq!(key_player(k), d);
            let p = key_payload(k);
            assert_eq!(p >> 45, j as u64, "the key's index");
            counts.extend((0..9).map(|i| ((p >> (5 * i)) & 31) as u8));
        }
        counts
    };
    let (mut checked, mut worlds_apart, mut whole) = (0, 0, 0);
    for seed in 0..scale(60) {
        let g = MtgAz::new(&s, seed).unwrap();
        let d = g.m.dealt.unwrap();
        let sb = g.sideboarding().unwrap();
        if sb.order[0] == d {
            continue;
        }
        let o = 1 - d as usize;
        assert_eq!(g.to_move(), Some(o));
        let n_keys = g.m.pools[d as usize].list.len().div_ceil(9);
        // the real step: the deal, exactly
        let mut real = g.clone();
        let mut keys = Vec::new();
        real.step(act::PASS, &mut keys).unwrap();
        assert_eq!(real.m.game_no, 2);
        assert_eq!(keys.iter().filter(|&&k| key_kind(k) == KEY_DEALT).count(), n_keys);
        assert_eq!(key_kind(*keys.last().unwrap()), KEY_GAME, "the deal's keys, then the next game's");
        let counts = decode(&keys, d);
        assert_eq!(&counts[..real.m.decks[d as usize].len()], &real.m.decks[d as usize][..], "seed {seed}: the keys are the dealt 60");
        assert!(counts[real.m.decks[d as usize].len()..].iter().all(|&c| c == 0));
        // the other seat's worlds: HIDDEN keys, equal across worlds whose deals differ
        let mut wk: Vec<Vec<u64>> = Vec::new();
        let mut wd = Vec::new();
        for w in 0..2u64 {
            let mut x = g.clone();
            x.determinize(o, &mut zero_search::rng::Rng::new(seed ^ w << 32 ^ 0x5EED));
            assert_eq!(x.m.dealt, Some(d), "every world keeps the dealt seat");
            let mut k = Vec::new();
            x.step(act::PASS, &mut k).unwrap();
            let dk: Vec<u64> = k.iter().copied().filter(|&k| key_kind(k) == KEY_DEALT).collect();
            assert_eq!(dk.len(), n_keys);
            assert!(dk.iter().all(|&k| key_payload(k) == HIDDEN && key_player(k) == d));
            wk.push(k);
            wd.push(x.m.decks[d as usize].clone());
        }
        assert_eq!(wk[0], wk[1], "seed {seed}: the other seat's worlds' keys");
        worlds_apart += (wd[0] != wd[1]) as usize;
        // the dealt seat's worlds: its later deal is chance not yet happened (reseeded), keyed in full
        let mut x = g.clone();
        x.determinize(d as usize, &mut zero_search::rng::Rng::new(seed ^ 0xD00D));
        let mut k = Vec::new();
        x.step(act::PASS, &mut k).unwrap();
        assert_eq!(&decode(&k, d)[..x.m.decks[d as usize].len()], &x.m.decks[d as usize][..]);
        checked += 1;
    }
    // a whole match (game 1 played): a dealt game-1 loser's deal is keyed in the step that ends game 1
    let whole_spec = spec_of(with(dd(1.0, 1, 8), json!({"start_game": null, "game_episodes": null, "max_turns": 12})));
    for seed in 0..scale(30) {
        let mut g = MtgAz::new(&whole_spec, seed).unwrap();
        let d = g.m.dealt.unwrap();
        let mut rng = Rng::new(seed ^ 0x3A);
        while g.m.game_no == 1 && g.sideboarding().is_none() && !g.match_over() {
            let l = legal(&g);
            let mut keys = Vec::new();
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
            if let Some(sb) = g.sideboarding() {
                if sb.order[0] == d {
                    assert_eq!(decode(&keys, d)[..g.m.decks[d as usize].len()], sb.done[d as usize].as_ref().unwrap()[..]);
                    whole += 1;
                } else {
                    assert!(keys.iter().all(|&k| key_kind(k) != KEY_DEALT));
                }
            }
        }
    }
    println!("{checked} deals keyed, {worlds_apart} with the other seat's two worlds dealing different 60s; {whole} in a whole match's game-ending step");
    assert!(checked > 10 && worlds_apart > checked / 2 && whole > 0);
}

/// The other player's worlds (`determinize`): the dealt seat's 60 - in a game, its last game's between games, its
/// next game's when it was dealt first - is a legal 60 of its 75 with swaps in the range, holding every card the
/// viewer saw this game; every world keeps the dealt seat; never tilted (a world dealt with a deck prediction is the
/// untilted world, draw for draw, and `deck_tilt_of` gives nothing), while the dealt seat's worlds of the other
/// player's 60 are tilted as any. With the K-sample deal off (`deal_samples` 0), the swap counts of the worlds' next
/// 60s are uniform over the range: game 3's deal ignores game 2's reveals.
#[test]
fn the_other_players_worlds_deal_the_dealt_60_from_the_restricted_prior_untilted() {
    let (main, _) = delver_ids();
    let main_s = sorted(main);
    let s = spec_of(with(dd(1.0, 1, 8), json!({"deal_samples": 0})));
    let s_k = spec_of(dd(1.0, 1, 8));
    let (mut worlds, mut in_game, mut changed, mut tilted_apart, mut own_tilted, mut g3, mut hidden_n) = (0, 0, 0, 0, 0, 0, 0);
    let mut next_hist = [0usize; 16];
    let mut g3_hist = [0usize; 16];
    let pred = |seed: u64| -> Vec<f32> {
        let mut r = Rng::new(seed ^ 0x9E3);
        let mut seg = || {
            let mut p: Vec<f32> = (0..D).map(|_| ((r.below(1000) + 1) as f32).powi(3)).collect();
            let t: f32 = p.iter().sum();
            p.iter_mut().for_each(|x| *x /= t);
            p
        };
        let mut v = seg();
        v.extend(seg());
        v
    };
    for (k, sp) in [&s, &s_k].into_iter().enumerate() {
        for seed in 0..scale(24) {
            let mut g = MtgAz::new(sp, seed).unwrap();
            let d = g.m.dealt.unwrap();
            let o = 1 - d;
            let (mut rng, mut h, mut keys) = (Rng::new(seed ^ 0x77), Heuristic::new(seed), Vec::new());
            for step in 0..2000 {
                let Some(_) = g.to_move() else {
                    match next(&g) {
                        Some(x) => {
                            g = x;
                            continue;
                        }
                        None => break,
                    }
                };
                let between = g.sideboarding().is_some();
                if between || step % 9 == 0 {
                    let mut x = g.clone();
                    x.determinize(o as usize, &mut zero_search::rng::Rng::new(seed ^ (step as u64) << 16));
                    assert_eq!(x.m.dealt, Some(d), "every world keeps the dealt seat");
                    let deck = x.m.deck(d);
                    assert_eq!(deck.len(), 60);
                    assert!(x.m.decks[d as usize].iter().zip(&x.m.pools[d as usize].list).all(|(&c, e)| c <= e.1));
                    if g.deck_hidden(d) {
                        assert!((1..=8).contains(&swaps(&main_s, &deck)), "seed {seed}: a world's dealt 60 of {} swaps", swaps(&main_s, &deck));
                        assert!(holds(&deck, &shown(&g, o)), "seed {seed}: a world's dealt 60 lacks a card the viewer saw");
                        changed += (deck != g.m.deck(d)) as usize;
                        hidden_n += 1;
                        in_game += !between as usize;
                    } else {
                        assert_eq!(deck, main_s, "game 1's deck is the registered main");
                    }
                    if let Some(sb) = x.sideboarding() {
                        if let Some(c) = &sb.done[d as usize] {
                            let nd = x.m.pools[d as usize].deck(c);
                            let n = swaps(&main_s, &nd);
                            assert!((1..=8).contains(&n), "seed {seed}: a world's next dealt 60 of {n} swaps");
                            if k == 0 {
                                next_hist[n] += 1;
                                if g.m.game_no == 2 {
                                    g3_hist[n] += 1;
                                    g3 += 1;
                                }
                            }
                        }
                    }
                    // never tilted: the deck-tilted world is the untilted one
                    let b = Belief { pred: pred(seed ^ step as u64), tilt: 0.0, clip: 1e6, tilts: vec![0.0, 1.0] };
                    let mut y = g.clone();
                    y.determinize_with(o as usize, &mut zero_search::rng::Rng::new(seed ^ (step as u64) << 16), &b);
                    assert_eq!((&y.m.decks, &y.m.pools), (&x.m.decks, &x.m.pools), "seed {seed} step {step}: a tilted world of the dealt seat's 60");
                    if !between {
                        assert_eq!(y.g.fingerprint(), x.g.fingerprint());
                        let t = DeckTilt { p: &b.pred[D..], tilt: 1.0, clip: 1e6 };
                        assert!(g.deck_tilt_of(o, t).is_none());
                        // the dealt seat's own worlds tilt the other player's 60 (sometimes another world)
                        if g.deck_hidden(o) {
                            let mut u = g.clone();
                            u.determinize(d as usize, &mut zero_search::rng::Rng::new(seed ^ 0xF00 ^ step as u64));
                            let mut v = g.clone();
                            v.determinize_with(d as usize, &mut zero_search::rng::Rng::new(seed ^ 0xF00 ^ step as u64), &b);
                            own_tilted += 1;
                            tilted_apart += (u.m.decks != v.m.decks) as usize;
                        }
                    }
                    worlds += 1;
                }
                let a = mixed(&g, &mut h, &mut rng);
                g.step(a, &mut keys).unwrap();
            }
        }
    }
    println!(
        "{worlds} worlds of the other seat ({in_game} in a dealt game, {changed} with another 60 than the real one); next 60s' swaps {next_hist:?} \
         ({g3} before game 3: {g3_hist:?}); the dealt seat's worlds tilted apart in {tilted_apart} of {own_tilted}"
    );
    assert!(in_game > 0 && changed * 2 > hidden_n && tilted_apart > 0 && g3 > 0);
    // the worlds' next 60s (dealt first, nothing shown): uniform over 1..=8, within 5 sd
    let n: usize = next_hist.iter().sum();
    let e = n as f64 / 8.0;
    let sd = (e * 7.0 / 8.0).sqrt();
    for (k, &c) in next_hist.iter().enumerate().take(9).skip(1) {
        assert!((c as f64 - e).abs() < 5.0 * sd + 2.0, "swaps {k}: {c} of {n} ({next_hist:?})");
    }
}

// ---- The dealt digest (ENGINE.md 154) ----

/// FNV-1a over 64-bit words (as digest.rs).
struct H(u64);

impl H {
    fn new() -> H {
        H(0xcbf2_9ce4_8422_2325)
    }
    fn u64(&mut self, x: u64) {
        for b in x.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn slice<T: Copy + Into<u64>>(&mut self, xs: &[T]) {
        self.u64(xs.len() as u64);
        for &x in xs {
            self.u64(x.into());
        }
    }
}

/// One match of the dealt Delver spec hashed: per decision the decider, the decision kind (a sideboarding's), the
/// legal actions, the action and the step's keys; every 10th decision the decider's whole row; every 20th its aux
/// targets and a world `determinize` deals it (both decks, the sideboarding's choices, its row there); each game's
/// end and the match's results. Random play, or the heuristic in seat `seed % 2` (`bot`).
fn digest_match(sp: &MtgSpec, seed: u64, bot: bool) -> u64 {
    let mut g = MtgAz::new(sp, seed).unwrap();
    let (mut rng, mut heur, mut keys) = (Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 7), Heuristic::new(seed ^ 0xB07), Vec::new());
    let mut h = H::new();
    h.u64(g.m.dealt.map_or(9, |d| d as u64));
    let mut step = 0u32;
    loop {
        let Some(p) = g.to_move() else {
            h.u64(0xE0E0);
            h.slice(&g.m.results);
            match next(&g) {
                Some(x) => {
                    g = x;
                    continue;
                }
                None => break,
            }
        };
        let l = legal(&g);
        h.u64(p as u64);
        h.u64(g.pending_decision().map_or(99, |d| d.kind as u64));
        h.slice(&l);
        if step % 10 == 0 {
            for x in row(&g, p) {
                h.u64(x.to_bits() as u64);
            }
        }
        if step % 20 == 7 {
            if g.sideboarding().is_none() {
                let mut out = vec![0.0f32; 2 * D];
                g.aux_target(p, &mut out);
                for x in out {
                    h.u64(x.to_bits() as u64);
                }
            }
            let mut w = g.clone();
            w.determinize(p, &mut zero_search::rng::Rng::new(seed ^ (step as u64) << 20));
            h.slice(&w.m.decks[0]);
            h.slice(&w.m.decks[1]);
            if let Some(sb) = w.sideboarding() {
                for d in &sb.done {
                    h.slice(d.as_deref().unwrap_or(&[]));
                }
            }
            for x in row(&w, p) {
                h.u64(x.to_bits() as u64);
            }
        }
        let a = if bot && p == (seed % 2) as usize && g.sideboarding().is_none() { heur.choose(&g.g, &l) } else { l[rng.below(l.len() as u32) as usize] };
        keys.clear();
        g.step(a, &mut keys).unwrap();
        h.u64(a as u64);
        h.slice(&keys);
        step += 1;
        assert!(step < 300_000, "match {seed}: does not end");
    }
    h.slice(&g.m.results);
    h.u64(g.m.game_no as u64);
    h.0
}

/// `cargo test --release -p engine --test dealt_decks -- --ignored --nocapture`: 60 matches of the dealt Delver spec
/// (the delver-mirror-bo3-dealt preset's: post-sideboard games, p 0.5, swaps [1, 8]; 40 random, 20 the heuristic
/// against random) and 20 whole matches (game 1 played, p 1, swaps [0, 15]) hashed against
/// `tests/data/digests_dealt.txt`; `DIGEST_WRITE=1` records it (only when behaviour is meant to change, stating old ->
/// new). The existing specs' digests are `digest.rs`'s (every game of the engine without the option).
#[test]
#[ignore]
fn dealt_digests() {
    let post = spec_of(dd(0.5, 1, 8));
    let whole = spec_of(with(dd(1.0, 0, 15), json!({"start_game": null, "game_episodes": null, "max_turns": 30})));
    let mut lines = Vec::new();
    let t = std::time::Instant::now();
    for seed in 0..40 {
        lines.push(format!("r {seed} {:016x}", digest_match(&post, seed, false)));
    }
    for seed in 0..20 {
        lines.push(format!("h {seed} {:016x}", digest_match(&post, seed, true)));
    }
    for seed in 0..20 {
        lines.push(format!("w {seed} {:016x}", digest_match(&whole, seed, false)));
    }
    let mut all = H::new();
    for l in &lines {
        for b in l.bytes() {
            all.u64(b as u64);
        }
    }
    println!("dealt_digests: {} matches in {:.1} s; combined digest {:016x}", lines.len(), t.elapsed().as_secs_f64(), all.0);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/digests_dealt.txt");
    if std::env::var("DIGEST_WRITE").is_ok() {
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        println!("wrote {}", path.display());
        return;
    }
    let recorded = std::fs::read_to_string(&path).expect("tests/data/digests_dealt.txt (record with DIGEST_WRITE=1)");
    let rec: Vec<&str> = recorded.lines().collect();
    let differ: Vec<&String> = lines.iter().zip(&rec).filter(|(a, b)| a.as_str() != **b).map(|(a, _)| a).collect();
    assert_eq!(rec.len(), lines.len(), "the recorded file's matches");
    assert!(differ.is_empty(), "{} of {} dealt matches differ from the recorded digests; first: {:?}", differ.len(), lines.len(), &differ[..differ.len().min(5)]);
}
