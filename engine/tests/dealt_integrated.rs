//! Dealt decks (ENGINE.md 154) with lane A1's posterior (140) and lane A2's worlds (141): the three tests the user
//! required at the integration (ENGINE.md 157.3).
//!
//! (1) A dealt seat's hidden 60 is the exact posterior of what the other player saw: the dealt
//! prior - the constructed prior with the swap count restricted to the spec's range (the user's [1, 8]: uniform over
//! the swap counts in the range, then uniform over the 60s with that count) - times prod_i C(D_i, R_i), the chance of
//! the seen cards R given the deck (140), and UNTILTED: no deck reader's weights (102.1), no deck tilt (106), since no
//! policy chose the 60 (154.2).
//!
//! * At the prior: a 75 small enough to list every legal deck, the request's law deck by deck against that exact
//!   posterior for two swap ranges and two sets of seen cards, and the exact posterior against the truth by looking
//!   (the dealt prior's decks shuffled, kept when their first cards are R).
//! * In the game: the Delver mirror's dealt matches (swaps [1, 8]), the other player's worlds of the dealt 60 at the
//!   start of game 2 (nothing of it seen: the exact law is the dealt prior's own) by `determinize`, by the deck
//!   reader's weights and by the deck tilt - all three the exact law; and later in the games, the reader's weights
//!   and the deck tilt deal the dealt 60 draw for draw as the untilted deal.
//!
//! (2) A dealt seat's game-3 60 is dealt from game 3's cards only - no game-2 term, no mixture with its dealt game-2
//! 60 (the user's decision: game 3's deal is independent of game 2's; `MtgAz::deck_dealt` is what any conditioning of
//! game 3 on game 2, ENGINE.md 141.4's options B-D, must check first). Today nothing conditions game 3 on game 2, so
//! this is the standing rule, checked where game 2 showed cards of the dealt 60.
//!
//! (3) A dealt deal keeps the world's observer: a world dealt while sideboarding (A2's 141.1) keys the dealt seat's
//! deal hidden for the other player and plays the next game with the other player's private keys masked.

use engine::az::constructed::Decklist;
use engine::az::hidden_decks::{check_sample, ConstructedPrior, DeckPrior, DeckWant, SampledDeck};
use engine::az::identity::D;
use engine::az::matches::game_one_winner;
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::decision::act;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DEALT, KEY_DRAW, KEY_LOOK};
use engine::rng::Rng;
use serde_json::json;
use zero_search::game::{Belief, Next, SearchGame};

fn db() -> &'static CardDb {
    CardDb::constructed()
}

fn count(deck: &[u16], d: u16) -> usize {
    deck.iter().filter(|&&x| x == d).count()
}

/// Pearson's chi-square of the dealt decks' histogram against the exact law `p` (A1's test, ENGINE.md 140.5): a
/// deck of probability 0 is never dealt; the statistic below df + 6 sd and no deck beyond 5.5 se.
fn chi2(label: &str, p: &[f64], hist: &[f64], n: usize) {
    let (mut x2, mut df, mut worst) = (0.0f64, 0usize, 0.0f64);
    for (i, (&pi, &h)) in p.iter().zip(hist).enumerate() {
        if pi == 0.0 {
            assert_eq!(h, 0.0, "{label}: deck {i} has probability 0 and was dealt");
            continue;
        }
        let ex = pi * n as f64;
        x2 += (h - ex) * (h - ex) / ex;
        df += 1;
        worst = f64::max(worst, ((h - ex) / ex.sqrt()).abs());
    }
    let df = (df - 1) as f64;
    println!("{label}: chi-square {x2:.1} on {df} df, worst deck {worst:.1} se");
    assert!(x2 < df + 6.0 * (2.0 * df).sqrt() && worst < 5.5, "{label}: chi-square {x2:.1} on {df} df, worst deck {worst:.1} se: not the exact posterior");
}

/// A 75 of a 10-card main (Brainstorm 4, Ponder 3, Lightning Bolt 3) and a 10-card sideboard (Lightning Bolt 1,
/// Pyroblast 3, Daze 3, Force of Will 3): 826 legal 10s, swap counts 0..=9, so the user's range [1, 8] cuts both
/// ends (the main itself, s = 0, and the three decks of s = 9) and [2, 5] more. For each range and two sets of
/// seen cards (main cards only, where the full prior would deal the main itself; and two Pyroblasts, sideboard
/// only): the request of `ConstructedPrior::dealt` (no tilt, no pick: a dealt 60's request) deck by deck against
/// P(D | R) proportional to [lo <= s(D) <= hi] / #{decks with s(D) swaps} x prod_i C(D_i, R_i), by `sample` and by
/// `sample_k`; `sample_k` equal to K `sample`s; `expected_fill` equal to the exact marginals; the exact posterior
/// equal to the truth by looking; and the full prior dealing decks outside the range for the same request (what the
/// restriction removes).
#[test]
fn a_dealt_60_is_the_restricted_prior_times_the_seen_cards_likelihood_exactly() {
    let db = db();
    let id = |s: &str| db.expect_id(s).unwrap();
    let (a, b, c, e, f, g) = (id("Brainstorm"), id("Ponder"), id("Lightning Bolt"), id("Pyroblast"), id("Daze"), id("Force of Will"));
    let main: Vec<u16> = [(a, 4), (b, 3), (c, 3)].iter().flat_map(|&(x, n)| std::iter::repeat_n(x, n)).collect();
    let side: Vec<u16> = [(c, 1), (e, 3), (f, 3), (g, 3)].iter().flat_map(|&(x, n)| std::iter::repeat_n(x, n)).collect();
    let full = ConstructedPrior::new(&main, &side);
    // the 75's entries (sorted by definition): (definition, copies in the main, copies in all)
    let mut list: Vec<(u16, u8, u8)> = vec![(a, 4, 4), (b, 3, 3), (c, 3, 4), (e, 0, 3), (f, 0, 3), (g, 0, 3)];
    list.sort_unstable();
    let mut decks: Vec<Vec<u8>> = Vec::new();
    let mut cur = vec![0u8; list.len()];
    fn rec(k: usize, left: i32, list: &[(u16, u8, u8)], cur: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if k == list.len() {
            if left == 0 {
                out.push(cur.clone());
            }
            return;
        }
        for x in 0..=list[k].2 {
            if (x as i32) <= left {
                cur[k] = x;
                rec(k + 1, left - x as i32, list, cur, out);
            }
        }
    }
    rec(0, 10, &list, &mut cur, &mut decks);
    assert_eq!(decks.len(), 826);
    let swaps = |v: &[u8]| -> usize { v.iter().zip(&list).map(|(&x, l)| x.saturating_sub(l.1) as usize).sum() };
    let mut by = [0.0f64; 11];
    for v in &decks {
        by[swaps(v)] += 1.0;
    }
    assert_eq!(&by[..], full.decks_by_swaps(), "the full prior's count of decks by swaps");
    assert_eq!((by[0], by[9], by[10]), (1.0, 3.0, 0.0));
    let index_of = |deck: &[u16]| -> usize {
        let v: Vec<u8> = list.iter().map(|l| count(deck, l.0) as u8).collect();
        decks.iter().position(|x| *x == v).expect("a listed deck")
    };
    let binom = |n: u8, k: u8| -> f64 { (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64) };
    let sorted = |mut v: Vec<u16>| {
        v.sort_unstable();
        v
    };
    let seen_sets = [sorted(vec![a, a, c]), sorted(vec![a, c, e, e])];
    let n = 200_000;
    let mut told_apart = 0usize;
    for (lo, hi) in [(1usize, 8usize), (2, 5)] {
        let prior = ConstructedPrior::dealt(&main, &side, lo, hi);
        assert_eq!(prior.swap_range(), (lo, hi));
        let want_by: Vec<f64> = (0..=10).map(|s| if (lo..=hi).contains(&s) { by[s] } else { 0.0 }).collect();
        assert_eq!(&want_by[..], prior.decks_by_swaps(), "[{lo}, {hi}]: the dealt prior's counts by swaps");
        for shown in &seen_sets {
            let r: Vec<u8> = list.iter().map(|l| count(shown, l.0) as u8).collect();
            // the exact posterior: the restricted prior (1 / (#s in range x #decks of s swaps)) x prod C(D_i, R_i)
            let mut p: Vec<f64> = decks
                .iter()
                .map(|v| {
                    let s = swaps(v);
                    if !(lo..=hi).contains(&s) {
                        return 0.0;
                    }
                    let mut w = 1.0 / ((hi - lo + 1) as f64 * by[s]);
                    for k in 0..list.len() {
                        if v[k] < r[k] {
                            return 0.0;
                        }
                        w *= binom(v[k], r[k]);
                    }
                    w
                })
                .collect();
            let z: f64 = p.iter().sum();
            p.iter_mut().for_each(|x| *x /= z);
            let label = format!("dealt [{lo}, {hi}], seen {shown:?}");
            // a dealt 60's request: untilted, no face-down pick (MtgAz::sample_hidden_deck, ENGINE.md 154.2)
            let want = DeckWant { db, n: 10, shown, companion: None, tilt: None, pick: None };
            let mut rng = Rng::new(0xDEA1 ^ (lo as u64) << 8 ^ shown.len() as u64);
            let mut h1 = vec![0.0f64; decks.len()];
            for _ in 0..n {
                let s = prior.sample(&want, &mut rng);
                check_sample(&prior, &want, &s).unwrap();
                h1[index_of(&s.deck)] += 1.0;
            }
            chi2(&format!("{label} by sample"), &p, &h1, n);
            let mut hk = vec![0.0f64; decks.len()];
            for s in prior.sample_k(&want, &[], n, &mut rng) {
                hk[index_of(&s.deck)] += 1.0;
            }
            chi2(&format!("{label} by sample_k"), &p, &hk, n);
            let (mut r1, mut r2) = (Rng::new(91), Rng::new(91));
            let one: Vec<SampledDeck> = (0..40).map(|_| prior.sample(&want, &mut r1)).collect();
            assert_eq!(prior.sample_k(&want, &[], 40, &mut r2), one, "{label}: sample_k is not the K samples");
            // the expected fill beyond R: the exact posterior's marginals
            let fill = prior.expected_fill(&want);
            for (k, l) in list.iter().enumerate() {
                let m: f64 = decks.iter().zip(&p).map(|(v, &pi)| pi * (v[k] - r[k].min(v[k])) as f64).sum();
                assert!((fill[l.0 as usize] - m).abs() < 1e-9, "{label}: {}'s fill {} against the exact {m}", db.def(l.0).name, fill[l.0 as usize]);
            }
            // the truth by looking: the dealt prior's decks with nothing shown, shuffled, kept when their first cards
            // are exactly R - the exact posterior is the posterior of the sightings
            let plain = DeckWant { db, n: 10, shown: &[], companion: None, tilt: None, pick: None };
            let k = shown.len();
            let mut hist = vec![0.0f64; decks.len()];
            let mut kept = 0usize;
            for _ in 0..40 {
                for s in prior.sample_k(&plain, &[], 50_000, &mut rng) {
                    let mut v = s.deck;
                    for i in 0..k {
                        let j = i + rng.below((v.len() - i) as u32) as usize;
                        v.swap(i, j);
                    }
                    let mut seen = v[..k].to_vec();
                    seen.sort_unstable();
                    if seen == *shown {
                        kept += 1;
                        hist[index_of(&v)] += 1.0;
                    }
                }
            }
            chi2(&format!("{label}: the truth by looking against the exact posterior"), &p, &hist, kept);
            // what the restriction removes: the full prior's posterior mass outside the range, exactly, and its
            // request's decks there (none where that mass is 0: the seen cards may already rule those decks out)
            let full_w: Vec<f64> = decks
                .iter()
                .map(|v| (0..list.len()).map(|k| if v[k] < r[k] { 0.0 } else { binom(v[k], r[k]) }).product::<f64>() / by[swaps(v)])
                .collect();
            let mass_out = full_w.iter().zip(&decks).filter(|(_, v)| !(lo..=hi).contains(&swaps(v))).map(|(w, _)| w).sum::<f64>()
                / full_w.iter().sum::<f64>();
            let m = n / 4;
            let outside = (0..m).filter(|_| !(lo..=hi).contains(&swaps(&decks[index_of(&full.sample(&want, &mut rng).deck)]))).count();
            let ex = mass_out * m as f64;
            println!("{label}: the full prior's request dealt {outside} of {m} decks outside [{lo}, {hi}] (exact {ex:.1})");
            assert!((outside as f64 - ex).abs() < 5.0 * ex.sqrt() + 1e-9, "{label}: the full prior's decks outside the range {outside}, exact {ex:.1}");
            told_apart += (outside > 0) as usize;
        }
    }
    // the user's range with main cards seen: the full prior would deal the main itself (s = 0)
    assert!(told_apart >= 2, "the full and the dealt priors' requests were told apart in {told_apart} cases: too few");
}

// ---- in the game: the Delver mirror's dealt matches ----

fn spec(extra: serde_json::Value) -> MtgSpec {
    let d = Decklist::izzet_delver();
    let main = d.main_names();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    let mut v = json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true, "start_game": 2,
        "decks": {"p0": main, "p1": main, "p0_sideboard": side, "p1_sideboard": side},
        "dealt_decks": {"p": 1.0, "swaps": [1, 8]}});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    MtgSpec::parse(&v.to_string()).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn next(g: &MtgAz) -> Option<MtgAz> {
    match g.next_episode(None).unwrap() {
        Next::Continue(n) => Some(n),
        Next::Over(_) => None,
    }
}

/// The swaps of `deck` against the registered main (both sorted).
fn swaps_of(main: &[u16], deck: &[u16]) -> usize {
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

/// Does `viewer` know any card of the other player's deck in this game (fuzz.rs's `hd_shown`, non-empty)?
fn knows_any(a: &MtgAz, viewer: u8) -> bool {
    let o = 1 - viewer;
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if o == 0 { 0..n0 } else { n0..n0 + n1 };
    r.clone().any(|i| a.g.knows(viewer, engine::CardIdx::new(i as u32))) || a.g.clouds().iter().any(|k| k.viewer == viewer && k.owner == o)
}

/// A peaked prediction over identities for both auxiliary segments (hand, deck): what a deck tilt would follow.
fn pred(seed: u64) -> Vec<f32> {
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
}

/// The other player's worlds of the dealt 60 at the start of game 2, where it has seen nothing of it (the dealt seat
/// sideboarded first or second, the other player kept its deck): `determinize`, `determinize_deck_tilted` by strong
/// reader weights (a sideboard card x30, a main card x0.05) and `determinize_with` by a peaked deck prediction (deck
/// tilt 1) all deal the dealt prior's exact law with nothing shown - each card's mean copies its `expected_fill`, the
/// swap counts uniform over 1..=8 - while the same weights on the prior's own draws would move the weighted card's
/// mean by far more than the test's resolution (fresh samples: `deal_samples` 0).
#[test]
fn the_other_players_worlds_deal_a_dealt_60_by_its_untilted_restricted_prior() {
    let db = db();
    let dl = Decklist::izzet_delver();
    let main = Decklist::ids(db, &dl.main).unwrap();
    let side = Decklist::ids(db, &dl.sideboard).unwrap();
    let mut main_s = main.clone();
    main_s.sort_unstable();
    let prior = ConstructedPrior::dealt(&main, &side, 1, 8);
    let plain = DeckWant { db, n: 60, shown: &[], companion: None, tilt: None, pick: None };
    let fill = prior.expected_fill(&plain);
    let heavy = *side.iter().find(|d| !main.contains(d)).expect("a sideboard card not in the main");
    let light = main[0];
    let w = move |d: u16| if d == heavy { 30.0 } else if d == light { 0.05 } else { 1.0 };
    let mut defs: Vec<u16> = main.iter().chain(&side).copied().collect();
    defs.sort_unstable();
    defs.dedup();
    let s = spec(json!({"deal_samples": 0}));
    let per = 1500usize;
    let seeds = 4u64;
    let total = per * seeds as usize;
    let mut sums = vec![[0.0f64; 3]; defs.len()];
    let mut sq = vec![[0.0f64; 3]; defs.len()];
    let mut swaps_hist = [[0usize; 16]; 3];
    for seed in 0..seeds {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let d = g.m.dealt.unwrap();
        let o = 1 - d;
        let mut keys = Vec::new();
        for _ in 0..200 {
            if g.sideboarding().is_none() {
                break;
            }
            let l = legal(&g);
            let a = if l.contains(&act::PASS) { act::PASS } else { l[0] };
            g.step(a, &mut keys).unwrap();
        }
        assert!(g.sideboarding().is_none(), "seed {seed}: the other player's sideboarding did not end");
        assert!(g.m.game_no == 2 && g.deck_hidden(d), "seed {seed}: game 2 in progress, the dealt 60 hidden");
        assert!(!knows_any(&g, o), "seed {seed}: the other player already knows a card of the dealt 60");
        let belief = Belief { pred: pred(seed), tilt: 0.0, clip: 1e6, tilts: vec![0.0, 1.0] };
        for k in 0..per {
            for mode in 0..3 {
                let mut x = g.clone();
                let mut r = zero_search::rng::Rng::new(seed << 40 ^ (k as u64) << 4 ^ mode as u64);
                match mode {
                    0 => x.determinize(o as usize, &mut r),
                    1 => x.determinize_deck_tilted(o as usize, &mut r, &w),
                    _ => x.determinize_with(o as usize, &mut r, &belief),
                }
                assert_eq!(x.m.dealt, Some(d));
                let deck = x.m.deck(d);
                swaps_hist[mode][swaps_of(&main_s, &deck)] += 1;
                for (j, &def) in defs.iter().enumerate() {
                    let c = count(&deck, def) as f64;
                    sums[j][mode] += c;
                    sq[j][mode] += c * c;
                }
            }
        }
    }
    let names = ["determinize", "the reader's weights", "the deck tilt"];
    for mode in 0..3 {
        let mut worst = 0.0f64;
        for (j, &def) in defs.iter().enumerate() {
            let m = sums[j][mode] / total as f64;
            let var = sq[j][mode] / total as f64 - m * m;
            let se = (var / total as f64).sqrt().max(1e-9);
            let z = (m - fill[def as usize]) / se;
            worst = f64::max(worst, z.abs());
            assert!(z.abs() < 4.5, "{}: {}'s mean copies {m:.4} against the dealt prior's exact {:.4} ({z:.1} se)", names[mode], db.def(def).name, fill[def as usize]);
        }
        let h = &swaps_hist[mode];
        assert!(h[0] == 0 && h[9..].iter().all(|&x| x == 0), "{}: swaps outside [1, 8]: {h:?}", names[mode]);
        let e = total as f64 / 8.0;
        let x2: f64 = h[1..=8].iter().map(|&c| (c as f64 - e).powi(2) / e).sum();
        println!("{}: {total} worlds, every card's mean within {worst:.1} se of the exact fill; swaps {:?}, chi-square {x2:.1} on 7 df", names[mode], &h[1..=8]);
        assert!(x2 < 7.0 + 6.0 * 14f64.sqrt(), "{}: the swap counts are not uniform over 1..=8: {h:?}", names[mode]);
    }
    // the test's power: the reader's weights on the prior's own draws (what a tilted deal would give)
    let tw = DeckWant { db, n: 60, shown: &[], companion: None, tilt: Some(&w), pick: None };
    let mut rng = Rng::new(0x71);
    let tilted: Vec<f64> = (0..total).map(|_| count(&prior.sample(&tw, &mut rng).deck, heavy) as f64).collect();
    let m = tilted.iter().sum::<f64>() / total as f64;
    let se = ((tilted.iter().map(|x| x * x).sum::<f64>() / total as f64 - m * m) / total as f64).sqrt();
    println!("{}: the dealt prior's mean copies {:.4}; tilted by the weights {m:.4} +- {se:.4}", db.def(heavy).name, fill[heavy as usize]);
    assert!((m - fill[heavy as usize]).abs() > 10.0 * se, "the weights do not move the prior's draws: the test could not see a tilted deal");
}

/// Later in the games (the dealt 60 partly seen; both K = 0 and the K-sample deal): the other player's worlds by the
/// deck reader's weights deal the dealt 60 draw for draw as by weights of 1, and by the deck tilt as `determinize`
/// - while in the dealt seat's own worlds of the other player's (chosen) 60 the same weights move the deal.
#[test]
fn the_reader_weights_and_the_deck_tilt_never_move_a_dealt_60() {
    let one = |_: u16| 1.0;
    let (mut compared, mut own, mut own_moved) = (0usize, 0usize, 0usize);
    for extra in [json!({"deal_samples": 0}), json!({})] {
        let s = spec(extra);
        for seed in 0..8u64 {
            let mut g = MtgAz::new(&s, seed).unwrap();
            let d = g.m.dealt.unwrap();
            let o = 1 - d;
            let dl = Decklist::izzet_delver();
            let side = Decklist::ids(db(), &dl.sideboard).unwrap();
            let main = Decklist::ids(db(), &dl.main).unwrap();
            let heavy = *side.iter().find(|x| !main.contains(x)).unwrap();
            let w = move |x: u16| if x == heavy { 30.0 } else if x == main[0] { 0.05 } else { 1.0 };
            let (mut rng, mut keys) = (Rng::new(seed ^ 0x5EE), Vec::new());
            for step in 0..1500u64 {
                if g.to_move().is_none() {
                    match next(&g) {
                        Some(x) => {
                            g = x;
                            continue;
                        }
                        None => break,
                    }
                }
                if g.sideboarding().is_none() && g.deck_hidden(d) && step % 11 == 0 {
                    let t = seed << 32 ^ step;
                    let belief = Belief { pred: pred(t), tilt: 0.0, clip: 1e6, tilts: vec![0.0, 1.0] };
                    let world = |f: &dyn Fn(&mut MtgAz, &mut zero_search::rng::Rng)| {
                        let mut x = g.clone();
                        f(&mut x, &mut zero_search::rng::Rng::new(t));
                        x
                    };
                    let w1 = world(&|x: &mut MtgAz, r: &mut zero_search::rng::Rng| x.determinize_deck_tilted(o as usize, r, &w));
                    let w0 = world(&|x: &mut MtgAz, r: &mut zero_search::rng::Rng| x.determinize_deck_tilted(o as usize, r, &one));
                    assert_eq!((&w1.m.decks, &w1.m.pools), (&w0.m.decks, &w0.m.pools), "seed {seed} step {step}: the reader's weights moved the dealt 60");
                    let b1 = world(&|x: &mut MtgAz, r: &mut zero_search::rng::Rng| x.determinize_with(o as usize, r, &belief));
                    let b0 = world(&|x: &mut MtgAz, r: &mut zero_search::rng::Rng| x.determinize(o as usize, r));
                    assert_eq!((&b1.m.decks, &b1.m.pools), (&b0.m.decks, &b0.m.pools), "seed {seed} step {step}: the deck tilt moved the dealt 60");
                    compared += 1;
                    if g.deck_hidden(o) {
                        let v1 = world(&|x: &mut MtgAz, r: &mut zero_search::rng::Rng| x.determinize_deck_tilted(d as usize, r, &w));
                        let v0 = world(&|x: &mut MtgAz, r: &mut zero_search::rng::Rng| x.determinize_deck_tilted(d as usize, r, &one));
                        own += 1;
                        own_moved += (v1.m.decks != v0.m.decks) as usize;
                    }
                }
                let l = legal(&g);
                let a = l[rng.below(l.len() as u32) as usize];
                g.step(a, &mut keys).unwrap();
            }
        }
    }
    println!("{compared} states compared; in the dealt seat's own worlds the weights moved the other 60 in {own_moved} of {own}");
    assert!(compared > 50 && own_moved > 0, "{compared} states compared, {own_moved} of {own} moved: the test saw too little");
}

/// The other player keeps its deck (`PASS`) until the sideboarding is over (the dealt seat decides nothing there).
fn finish_sideboarding(g: &mut MtgAz, keys: &mut Vec<u64>, what: &str) {
    for _ in 0..200 {
        if g.sideboarding().is_none() {
            return;
        }
        let l = legal(g);
        let a = if l.contains(&act::PASS) { act::PASS } else { l[0] };
        g.step(a, keys).unwrap();
    }
    panic!("{what}: the sideboarding did not end");
}

/// A random legal action.
fn random_step(g: &mut MtgAz, rng: &mut Rng, keys: &mut Vec<u64>) {
    let l = legal(g);
    let a = l[rng.below(l.len() as u32) as usize];
    keys.clear();
    g.step(a, keys).unwrap();
}

/// How many cards of the other player's deck `viewer` knows in this game.
fn known_count(a: &MtgAz, viewer: u8) -> usize {
    let o = 1 - viewer;
    let n0: usize = a.m.decks[0].iter().map(|&c| c as usize).sum();
    let n1: usize = a.m.decks[1].iter().map(|&c| c as usize).sum();
    let r = if o == 0 { 0..n0 } else { n0..n0 + n1 };
    r.filter(|&i| a.g.knows(viewer, engine::CardIdx::new(i as u32))).count()
}

/// Tallies of the other player's worlds of a dealt 60 against the dealt prior with nothing shown.
struct Tally {
    defs: Vec<u16>,
    sum: Vec<f64>,
    sq: Vec<f64>,
    swaps: [usize; 16],
    n: usize,
    same: usize,
    same_expected: f64,
}

impl Tally {
    fn new(defs: &[u16]) -> Tally {
        Tally { defs: defs.to_vec(), sum: vec![0.0; defs.len()], sq: vec![0.0; defs.len()], swaps: [0; 16], n: 0, same: 0, same_expected: 0.0 }
    }

    /// One world's dealt 60 `deck`; `before`: the dealt seat's game-2 60, whose chance under the dealt prior is
    /// `p_before`.
    fn add(&mut self, main_s: &[u16], deck: &[u16], before: &[u16], p_before: f64) {
        for (j, &d) in self.defs.iter().enumerate() {
            let x = count(deck, d) as f64;
            self.sum[j] += x;
            self.sq[j] += x * x;
        }
        self.swaps[swaps_of(main_s, deck)] += 1;
        self.n += 1;
        self.same += (deck == before) as usize;
        self.same_expected += p_before;
    }

    /// Every card's mean copies within 4.5 se of the dealt prior's exact fill with nothing shown, the swap counts
    /// uniform over 1..=8 and never outside, and as many worlds dealing the game-2 60 again as its chance gives.
    fn check(&self, label: &str, fill: &[f64], db: &CardDb) {
        let n = self.n as f64;
        let mut worst = 0.0f64;
        for (j, &d) in self.defs.iter().enumerate() {
            let m = self.sum[j] / n;
            let se = ((self.sq[j] / n - m * m) / n).sqrt().max(1e-9);
            let z = (m - fill[d as usize]) / se;
            worst = f64::max(worst, z.abs());
            assert!(z.abs() < 4.5, "{label}: {}'s mean copies {m:.4} against the dealt prior's {:.4} with nothing shown ({z:.1} se)", db.def(d).name, fill[d as usize]);
        }
        let h = &self.swaps;
        assert!(h[0] == 0 && h[9..].iter().all(|&x| x == 0), "{label}: swaps outside [1, 8]: {h:?}");
        let e = n / 8.0;
        let x2: f64 = h[1..=8].iter().map(|&c| (c as f64 - e).powi(2) / e).sum();
        assert!(x2 < 7.0 + 6.0 * 14f64.sqrt(), "{label}: the swap counts are not uniform over 1..=8: {h:?}");
        let ex = self.same_expected;
        println!(
            "{label}: {} worlds, every card's mean within {worst:.1} se of the dealt prior's fill; swaps {:?} (chi-square {x2:.1} on 7 df); \
             the game-2 60 dealt again {} times (its chance gives {ex:.2})",
            self.n,
            &h[1..=8],
            self.same
        );
        assert!((self.same as f64) <= ex + 5.0 * ex.sqrt() + 3.0, "{label}: the game-2 60 dealt again {} times, its chance {ex:.2}: a mixture with game 2's deck", self.same);
    }
}

/// (2) The Delver mirror's dealt matches (swaps [1, 8], fresh samples), game 2 played for 150 random actions (the
/// other player sees cards of the dealt game-2 60), then conceded by game 1's winner: 1-1, a game 3. The other
/// player's worlds of the dealt seat's game-3 60 - at the sideboarding before game 3 when the dealt seat sideboarded
/// first (its game-3 60 dealt: `deal_constructed_between`'s choice with nothing shown), and at game 3's start (nothing
/// of it seen this game: `sample_hidden_deck`), by `determinize` and by the deck tilt - are the dealt prior's law with
/// nothing shown: every card's mean copies its `expected_fill`, the swaps uniform over 1..=8, the game-2 60 dealt
/// again no more often than its own chance. Conditioning on game 2's sightings, or a mixture with the game-2 deck
/// (141.4's B-D), would move the cards game 2 showed and deal the game-2 60 again in a share of the worlds.
#[test]
fn a_dealt_seats_game_3_60_is_dealt_from_game_3s_cards_only() {
    let db = db();
    let dl = Decklist::izzet_delver();
    let main = Decklist::ids(db, &dl.main).unwrap();
    let side = Decklist::ids(db, &dl.sideboard).unwrap();
    let mut main_s = main.clone();
    main_s.sort_unstable();
    let prior = ConstructedPrior::dealt(&main, &side, 1, 8);
    let plain = DeckWant { db, n: 60, shown: &[], companion: None, tilt: None, pick: None };
    let fill = prior.expected_fill(&plain);
    let by = prior.decks_by_swaps().to_vec();
    let mut defs: Vec<u16> = main.iter().chain(&side).copied().collect();
    defs.sort_unstable();
    defs.dedup();
    let s = spec(json!({"deal_samples": 0}));
    let per = 1000usize;
    let (mut between, mut start) = (Tally::new(&defs), Tally::new(&defs));
    let (mut seeds_between, mut seeds_start, mut seen_in_game_2) = (0, 0, 0usize);
    for seed in 0..24u64 {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let d = g.m.dealt.unwrap();
        let o = 1 - d;
        let (mut rng, mut keys) = (Rng::new(seed ^ 0x6A3E), Vec::new());
        finish_sideboarding(&mut g, &mut keys, &format!("seed {seed}, before game 2"));
        for _ in 0..150 {
            if g.to_move().is_none() {
                break;
            }
            random_step(&mut g, &mut rng, &mut keys);
        }
        if g.to_move().is_none() || g.m.game_no != 2 {
            continue; // (game 2 ended on its own: the concession below needs it in progress)
        }
        let deck2 = g.m.deck(d);
        let p2 = 1.0 / (8.0 * by[swaps_of(&main_s, &deck2)]);
        seen_in_game_2 += known_count(&g, o);
        let w = game_one_winner(seed) as usize;
        g = match g.next_episode(Some(w)).unwrap() {
            Next::Continue(n) => n,
            Next::Over(_) => panic!("seed {seed}: 1-1 after game 2"),
        };
        assert!(g.sideboarding().is_some(), "seed {seed}: the sideboarding before game 3");
        let belief = Belief { pred: pred(seed), tilt: 0.0, clip: 1e6, tilts: vec![0.0, 1.0] };
        if g.sideboarding().unwrap().done[d as usize].is_some() {
            // the dealt seat sideboarded first: its game-3 60 is dealt; the other player's worlds deal it again
            for k in 0..per {
                let mut x = g.clone();
                x.determinize(o as usize, &mut zero_search::rng::Rng::new(seed << 32 ^ k as u64));
                let c = x.sideboarding().unwrap().done[d as usize].clone().expect("the dealt seat's game-3 60");
                between.add(&main_s, &x.m.pools[d as usize].deck(&c), &deck2, p2);
            }
            seeds_between += 1;
        }
        finish_sideboarding(&mut g, &mut keys, &format!("seed {seed}, before game 3"));
        assert!(g.m.game_no == 3 && g.deck_hidden(d), "seed {seed}: game 3 in progress, the dealt 60 hidden");
        assert!(!knows_any(&g, o), "seed {seed}: the other player knows a card of the game-3 60 at its start");
        for k in 0..per {
            let mut x = g.clone();
            let mut r = zero_search::rng::Rng::new(seed << 32 ^ 0x8000_0000 ^ k as u64);
            if k % 2 == 0 {
                x.determinize(o as usize, &mut r);
            } else {
                x.determinize_with(o as usize, &mut r, &belief);
            }
            start.add(&main_s, &x.m.deck(d), &deck2, p2);
        }
        seeds_start += 1;
    }
    println!("{seeds_start} games 3 ({seeds_between} with the dealt seat sideboarding first); the other player knew {seen_in_game_2} cards of the dealt game-2 60s when game 2 was conceded");
    assert!(seeds_start >= 8 && seeds_between >= 2 && seen_in_game_2 > 0, "too few games 3 or nothing seen in game 2");
    between.check("the dealt seat's game-3 choice in the other player's worlds (between games)", &fill, db);
    start.check("the dealt seat's game-3 60 at game 3's start", &fill, db);
}

/// (3) Worlds dealt while sideboarding - before game 2 and before game 3, of both seats - walked into the next game:
/// the dealt seat's deal in the world (`KEY_DEALT`, when it sideboards second) is keyed `HIDDEN` in the other
/// player's worlds and in full in its own; the world's observer is carried into the game it starts (A2's 141.1), so
/// every draw and look of the other player there is keyed `HIDDEN` (but a look at a random card of the viewer's own
/// hand, Urza's Bauble: public by design, 141.2).
#[test]
fn a_dealt_deal_keeps_the_worlds_observer() {
    let s = spec(json!({}));
    let (mut worlds, mut dealt_hidden, mut dealt_full, mut new_games, mut private, mut hand_looks) = (0, 0, 0, 0, 0, 0);
    for seed in 0..10u64 {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let d = g.m.dealt.unwrap();
        let o = 1 - d;
        let (mut rng, mut keys) = (Rng::new(seed ^ 0x0B5E), Vec::new());
        for root in 0..2 {
            if root == 1 {
                finish_sideboarding(&mut g, &mut keys, &format!("seed {seed}, before game 2"));
                for _ in 0..80 {
                    if g.to_move().is_none() {
                        break;
                    }
                    random_step(&mut g, &mut rng, &mut keys);
                }
                if g.to_move().is_none() || g.m.game_no != 2 {
                    break;
                }
                let w = game_one_winner(seed) as usize;
                g = match g.next_episode(Some(w)).unwrap() {
                    Next::Continue(n) => n,
                    Next::Over(_) => panic!("seed {seed}: 1-1 after game 2"),
                };
            }
            assert!(g.sideboarding().is_some());
            for viewer in [o, d] {
                for k in 0..4u64 {
                    let mut w = g.clone();
                    w.determinize(viewer as usize, &mut zero_search::rng::Rng::new(seed << 40 ^ (root as u64) << 32 ^ (viewer as u64) << 8 ^ k));
                    let mut wr = Rng::new(seed ^ k << 20 ^ 0x51DE);
                    let (mut started, mut since) = (false, 0);
                    let mut wk = Vec::new();
                    for _ in 0..400 {
                        if w.to_move().is_none() {
                            break;
                        }
                        let before = w.sideboarding().is_some();
                        let l = legal(&w);
                        let a = if before && l[0] == act::PASS && wr.below(2) == 0 { act::PASS } else { l[wr.below(l.len() as u32) as usize] };
                        wk.clear();
                        w.step(a, &mut wk).unwrap();
                        let what = || format!("seed {seed} root {root}: a world of P{viewer} (P{d} dealt)");
                        for &key in &wk {
                            let (kind, p, x) = (key_kind(key), key_player(key), key_payload(key));
                            if kind == KEY_DEALT {
                                assert_eq!(p, d, "{}: a deal key of the other seat", what());
                                if viewer == o {
                                    assert_eq!(x, HIDDEN, "{}: the dealt seat's deal keyed in full in the other player's world", what());
                                    dealt_hidden += 1;
                                } else {
                                    dealt_full += 1;
                                }
                            }
                            if started && matches!(kind, KEY_DRAW | KEY_LOOK) && p != viewer {
                                private += 1;
                                if x != HIDDEN {
                                    let hand_look = kind == KEY_LOOK && w.g.player(viewer).hand.iter().any(|&c| w.g.obj(c).def as u64 == x && w.g.knows(p, c));
                                    assert!(hand_look, "{}: in the game it started P{p}'s private key (kind {kind}) carries the card ({x})", what());
                                    hand_looks += 1;
                                }
                            }
                        }
                        if before && w.sideboarding().is_none() {
                            started = true;
                            new_games += 1;
                        } else if started {
                            since += 1;
                            if since >= 60 {
                                break;
                            }
                        }
                    }
                    assert!(started, "seed {seed} root {root}: the world of P{viewer} never started its next game");
                    worlds += 1;
                }
            }
        }
    }
    println!(
        "{worlds} worlds dealt while sideboarding: the dealt seat's deal keyed hidden {dealt_hidden} times in the other player's, in full {dealt_full} in its own; \
         {new_games} games started, {private} private keys of the other player there masked ({hand_looks} public hand looks)"
    );
    assert!(dealt_hidden > 0 && dealt_full > 0 && new_games == worlds && private > 0, "the test saw too little");
}
