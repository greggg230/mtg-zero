//! The hidden deck's posterior given the cards the viewer SAW (ENGINE.md 140; the Fable review's H1).
//!
//! The deal's model: a deck D from the prior, shuffled uniformly; the viewer has seen the identities of some of
//! its physical cards (R: `sample::knowledge`'s `required`, each one a distinct card of the deck), and which cards
//! it saw did not depend on what they are - the same model as the uniform deal of the rest. Under it
//! P(saw R | D) is proportional to prod_i C(D_i, R_i): a seen Lightning Bolt is likelier from a 60 with four Bolts
//! than from one with two. The posterior is P(D) prod_i C(D_i, R_i), not P(D) [D holds R].
//!
//! Each test compares a prior's request with the truth: decks of the prior with nothing shown, shuffled, their
//! first k cards looked at, kept when those are exactly R (or, on a 75 small enough to list every 60, the exact
//! posterior by enumeration). The constructed tests fail on 267ae603 (the request was P(D) [D holds R]: two
//! Bolts seen, 3.67 Bolts against a true 3.85); the table prior's pass there too (its fill is independent draws,
//! so R plus a fresh fill is already the posterior of what was seen).

use engine::az::constructed::Decklist;
use engine::az::hidden_decks::{check_sample, seen_weight, sighting_weight, ConstructedPrior, DealCache, DeckPrior, DeckWant, PickWant, SampledDeck, Seen, TablePrior};
use engine::db::CardDb;
use engine::rng::Rng;

fn cdb() -> &'static CardDb {
    CardDb::constructed()
}

/// The Delver 75 (main, sideboard) and its prior.
fn delver_prior() -> (Vec<u16>, Vec<u16>, ConstructedPrior) {
    let dl = Decklist::izzet_delver();
    let main = Decklist::ids(cdb(), &dl.main).unwrap();
    let side = Decklist::ids(cdb(), &dl.sideboard).unwrap();
    assert_eq!((main.len(), side.len()), (60, 15));
    let prior = ConstructedPrior::new(&main, &side);
    (main, side, prior)
}

fn count(deck: &[u16], d: u16) -> usize {
    deck.iter().filter(|&&x| x == d).count()
}

/// The truth by simulation: `draw(n)` gives n decks of the prior with nothing shown; each is shuffled (its first
/// `shown.len()` places, a partial Fisher-Yates) and kept when those cards are exactly `shown`. Returns the kept
/// decks' histogram of `card`'s copies (0..=8) and the number kept.
fn truth_by_looking(draw: &mut dyn FnMut(usize, &mut Rng) -> Vec<SampledDeck>, shown: &[u16], card: u16, sims: usize, rng: &mut Rng) -> ([f64; 9], usize) {
    let k = shown.len();
    let mut hist = [0.0f64; 9];
    let mut kept = 0usize;
    let mut done = 0;
    while done < sims {
        let chunk = (sims - done).min(20_000);
        for s in draw(chunk, rng) {
            let mut d = s.deck;
            for i in 0..k {
                let j = i + rng.below((d.len() - i) as u32) as usize;
                d.swap(i, j);
            }
            let mut seen = d[..k].to_vec();
            seen.sort_unstable();
            if seen == shown {
                kept += 1;
                hist[count(&d, card)] += 1.0;
            }
        }
        done += chunk;
    }
    (hist, kept)
}

/// Mean and its standard error of a histogram of counts with `n` observations.
fn mean_se(hist: &[f64; 9], n: usize) -> (f64, f64) {
    let n = n as f64;
    let m: f64 = hist.iter().enumerate().map(|(c, &h)| c as f64 * h).sum::<f64>() / n;
    let m2: f64 = hist.iter().enumerate().map(|(c, &h)| (c * c) as f64 * h).sum::<f64>() / n;
    (m, ((m2 - m * m) / n).sqrt())
}

/// The request's copies of `card` (0..=8) in `n` samples.
fn request_hist(prior: &dyn DeckPrior, want: &DeckWant, card: u16, n: usize, rng: &mut Rng) -> [f64; 9] {
    let mut hist = [0.0f64; 9];
    for _ in 0..n {
        let s = prior.sample(want, rng);
        check_sample(prior, want, &s).unwrap();
        hist[count(&s.deck, card)] += 1.0;
    }
    hist
}

/// The two histograms agree: the means within 4.5 combined standard errors, and every count's share too (a
/// two-proportion test, the shares pooled: both samples come from one law when the request is the posterior).
fn assert_same_law(label: &str, truth: &[f64; 9], kept: usize, req: &[f64; 9], n: usize) {
    let (mt, st) = mean_se(truth, kept);
    let (mr, sr) = mean_se(req, n);
    let z = (mt - mr) / (st * st + sr * sr).sqrt();
    println!("{label}: TRUE posterior {mt:.4} +- {st:.4} ({kept} kept); the request {mr:.4} +- {sr:.4}; {z:.1} se");
    assert!(z.abs() < 4.5, "{label}: the request is not the posterior of the seen cards: truth {mt:.4} +- {st:.4}, request {mr:.4} +- {sr:.4} ({z:.1} se)");
    for c in 0..9 {
        let (p, q) = (truth[c] / kept as f64, req[c] / n as f64);
        let pooled = (truth[c] + req[c]) / (kept + n) as f64;
        if pooled == 0.0 {
            continue;
        }
        let se = (pooled * (1.0 - pooled) * (1.0 / kept as f64 + 1.0 / n as f64)).sqrt();
        assert!((p - q).abs() < 4.5 * se, "{label}: {c} copies: truth {p:.4}, request {q:.4} ({:.1} se)", (p - q) / se);
    }
}

/// The Delver 75 (the review's probe, made a test): the request of a 60 given seen cards against the truth by
/// looking at the first cards of the plain prior's shuffled decks. On 267ae603: one Pyroblast seen (0 main, 3
/// side) 2.27 copies against a true 2.56, two Lightning Bolts (4 main) 3.67 against 3.85 (27-38 se).
#[test]
fn the_hidden_60_is_the_posterior_of_the_cards_the_viewer_saw() {
    let db = cdb();
    let (_, _, prior) = delver_prior();
    let id = |s: &str| db.expect_id(s).unwrap();
    let plain = DeckWant { db, n: 60, shown: &[], companion: None, tilt: None, pick: None, seen: None };
    let mut draw = |n: usize, rng: &mut Rng| prior.sample_k(&plain, &[], n, rng);
    for (label, shown, card, sims) in [
        ("one Pyroblast seen (0 main, 3 side)", vec![id("Pyroblast")], id("Pyroblast"), 300_000usize),
        ("two Lightning Bolts seen (4 main)", vec![id("Lightning Bolt"), id("Lightning Bolt")], id("Lightning Bolt"), 1_000_000),
        ("a Pyroblast and an Unholy Heat seen (Heat: 2 main, 1 side)", vec![id("Pyroblast"), id("Unholy Heat")], id("Unholy Heat"), 3_000_000),
    ] {
        let mut shown = shown;
        shown.sort_unstable();
        let mut rng = Rng::new(0x140 ^ shown.len() as u64 ^ (card as u64) << 16);
        let (truth, kept) = truth_by_looking(&mut draw, &shown, card, sims, &mut rng);
        let want = DeckWant { db, n: 60, shown: &shown, companion: None, tilt: None, pick: None, seen: None };
        let n = 40_000;
        let req = request_hist(&prior, &want, card, n, &mut rng);
        assert_same_law(label, &truth, kept, &req, n);
        // the deck reader's baseline is the same posterior's mean (exact, by the tables)
        let fill = prior.expected_fill(&want)[card as usize] + count(&shown, card) as f64;
        let (mr, sr) = mean_se(&req, n);
        assert!((fill - mr).abs() < 4.5 * sr, "{label}: expected_fill {fill:.4} against the request's {mr:.4} +- {sr:.4}");
    }
}

/// A 75 small enough to list every legal deck (a main of 12, a sideboard of 4): the request's law against the exact
/// posterior P_base(D) prod C(D_i, R_i), times the tilt per copy beyond R and a face-down pick's chance a + b y,
/// deck by deck (chi-square), by `sample` and by the one-table `sample_k`; `expected_fill` against the exact
/// marginals; and the exact posterior against the truth by looking (the formula is the simulated one).
#[test]
fn a_small_75_deals_the_exact_posterior_of_the_seen_cards() {
    let db = cdb();
    let id = |s: &str| db.expect_id(s).unwrap();
    let (a, b, c, d, e, f) = (id("Brainstorm"), id("Ponder"), id("Lightning Bolt"), id("Daze"), id("Pyroblast"), id("Karakas"));
    let main: Vec<u16> = [(a, 4), (b, 4), (c, 2), (d, 2)].iter().flat_map(|&(x, n)| std::iter::repeat_n(x, n)).collect();
    let side: Vec<u16> = [(c, 1), (e, 2), (f, 1)].iter().flat_map(|&(x, n)| std::iter::repeat_n(x, n)).collect();
    let prior = ConstructedPrior::new(&main, &side);
    // the 75's entries (sorted by definition) and their copies in the main and in all
    let mut list: Vec<(u16, u8, u8)> = vec![(a, 4, 4), (b, 4, 4), (c, 2, 3), (d, 2, 2), (e, 0, 2), (f, 0, 1)];
    list.sort_unstable();
    // every legal 12 (count vectors), with its swaps
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
    rec(0, 12, &list, &mut cur, &mut decks);
    let swaps = |v: &[u8]| -> usize { v.iter().zip(&list).map(|(&x, l)| x.saturating_sub(l.1) as usize).sum() };
    let mut by = [0.0f64; 5];
    for v in &decks {
        by[swaps(v)] += 1.0;
    }
    assert_eq!(&by[..], prior.decks_by_swaps(), "the base prior's count of decks by swaps");
    let deck_of = |v: &[u8]| -> Vec<u16> { list.iter().zip(v).flat_map(|(l, &x)| std::iter::repeat_n(l.0, x as usize)).collect() };
    let index_of = |deck: &[u16]| -> usize {
        let v: Vec<u8> = list.iter().map(|l| count(deck, l.0) as u8).collect();
        decks.iter().position(|x| *x == v).expect("a listed deck")
    };
    let binom = |n: u8, k: u8| -> f64 { (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64) };
    let mut shown = vec![b, b, c, e];
    shown.sort_unstable();
    let r: Vec<u8> = list.iter().map(|l| count(&shown, l.0) as u8).collect();
    let tilt_of = |x: u16| -> f64 { if x == a { 3.0 } else if x == d { 0.4 } else if x == f { 2.0 } else { 1.0 } };
    let legal = {
        let mut v = vec![c, e];
        v.sort_unstable();
        v
    };
    let pw = PickWant { legal: legal.clone(), a: 1.0, b: 2.0 };
    // the exact posterior of a request: base x prod C(D_i, R_i) x tilt^(D_i - R_i) x (a + b y)
    let exact = |tilted: bool, pick: bool| -> Vec<f64> {
        let mut p: Vec<f64> = decks
            .iter()
            .map(|v| {
                let mut w = 1.0 / (5.0 * by[swaps(v)]);
                let mut y = 0usize;
                for (k, l) in list.iter().enumerate() {
                    if v[k] < r[k] {
                        return 0.0;
                    }
                    w *= binom(v[k], r[k]);
                    if tilted {
                        w *= tilt_of(l.0).powi((v[k] - r[k]) as i32);
                    }
                    if legal.contains(&l.0) {
                        y += (v[k] - r[k]) as usize;
                    }
                }
                if pick {
                    w *= pw.a + pw.b * y as f64;
                }
                w
            })
            .collect();
        let z: f64 = p.iter().sum();
        p.iter_mut().for_each(|x| *x /= z);
        p
    };
    let chi2 = |label: &str, p: &[f64], hist: &[f64], n: usize| {
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
    };
    let n = 200_000;
    for (tilted, pick) in [(false, false), (true, false), (false, true), (true, true)] {
        let label = format!("request (tilt {tilted}, pick {pick})");
        let want = DeckWant { db, n: 12, shown: &shown, companion: None, tilt: if tilted { Some(&tilt_of) } else { None }, pick: if pick { Some(&pw) } else { None }, seen: None };
        let p = exact(tilted, pick);
        let mut rng = Rng::new(0x5A11 ^ (tilted as u64) << 1 ^ pick as u64);
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
        // `sample_k` is K `sample`s, draw for draw (ENGINE.md 140: one table per request)
        let (mut r1, mut r2) = (Rng::new(77), Rng::new(77));
        let one: Vec<SampledDeck> = (0..40).map(|_| prior.sample(&want, &mut r1)).collect();
        assert_eq!(prior.sample_k(&want, &[], 40, &mut r2), one, "{label}: sample_k is not the K samples");
        // the expected fill: the untilted posterior's marginals beyond R, exactly
        if !tilted {
            let fill = prior.expected_fill(&want);
            for (k, l) in list.iter().enumerate() {
                let m: f64 = decks.iter().zip(&p).map(|(v, &pi)| pi * (v[k] - r[k].min(v[k])) as f64).sum();
                assert!((fill[l.0 as usize] - m).abs() < 1e-9, "{label}: {}'s fill {} against the exact {m}", db.def(l.0).name, fill[l.0 as usize]);
            }
        }
    }
    // The exact posterior is the truth by looking: plain decks, shuffled, the first four kept when they are R.
    let plain = DeckWant { db, n: 12, shown: &[], companion: None, tilt: None, pick: None, seen: None };
    let mut rng = Rng::new(0x100C);
    let mut hist = vec![0.0f64; decks.len()];
    let mut kept = 0usize;
    for _ in 0..40 {
        for s in prior.sample_k(&plain, &[], 50_000, &mut rng) {
            let mut v = s.deck;
            for i in 0..4 {
                let j = i + rng.below((v.len() - i) as u32) as usize;
                v.swap(i, j);
            }
            let mut seen = v[..4].to_vec();
            seen.sort_unstable();
            if seen == shown {
                kept += 1;
                hist[index_of(&v)] += 1.0;
            }
        }
    }
    chi2("the truth by looking against the exact posterior", &exact(false, false), &hist, kept);
    // `seen_weight` (the likelihood a caller can read, ENGINE.md 140): prod C(D_i, R_i)
    for v in &decks {
        let deck = deck_of(v);
        let w: f64 = (0..list.len()).map(|k| if v[k] < r[k] { 0.0 } else { binom(v[k], r[k]) }).product();
        assert!((seen_weight(&deck, &shown) - w).abs() < 1e-12);
    }
    // ENGINE.md 140.9: how the cards were seen. The two Ponders: a search's find first, then one seen blind; the
    // Bolt: a search's find; the Pyroblast: no sighting (a copy held unseen, as a payment's pinned card). The exact
    // posterior: base x per card the product over its blind sightings j of (D - j) x [D >= R] (x tilt x pick).
    let mut seen = vec![Seen { def: b, n: 2, searched: 0b01 }, Seen { def: c, n: 1, searched: 0b1 }];
    seen.sort_by_key(|x| x.def);
    let exact_seen = |tilted: bool, pick: bool| -> Vec<f64> {
        let mut p: Vec<f64> = decks
            .iter()
            .map(|v| {
                let mut w = 1.0 / (5.0 * by[swaps(v)]);
                let mut y = 0usize;
                for (k, l) in list.iter().enumerate() {
                    if v[k] < r[k] {
                        return 0.0;
                    }
                    if l.0 == b {
                        w *= v[k] as f64 - 1.0; // the blind one, after the find
                    } else if l.0 == a || l.0 == d || l.0 == f {
                        w *= binom(v[k], r[k]); // (none shown: 1)
                    }
                    if tilted {
                        w *= tilt_of(l.0).powi((v[k] - r[k]) as i32);
                    }
                    if legal.contains(&l.0) {
                        y += (v[k] - r[k]) as usize;
                    }
                }
                if pick {
                    w *= pw.a + pw.b * y as f64;
                }
                w
            })
            .collect();
        let z: f64 = p.iter().sum();
        p.iter_mut().for_each(|x| *x /= z);
        p
    };
    for (tilted, pick) in [(false, false), (true, false), (false, true), (true, true)] {
        let label = format!("request with sightings (tilt {tilted}, pick {pick})");
        let want = DeckWant { db, n: 12, shown: &shown, companion: None, tilt: if tilted { Some(&tilt_of) } else { None }, pick: if pick { Some(&pw) } else { None }, seen: Some(&seen) };
        let p = exact_seen(tilted, pick);
        let mut rng = Rng::new(0x5EE ^ (tilted as u64) << 1 ^ pick as u64);
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
        if !tilted {
            let fill = prior.expected_fill(&want);
            for (k, l) in list.iter().enumerate() {
                let m: f64 = decks.iter().zip(&p).map(|(v, &pi)| pi * (v[k] - r[k].min(v[k])) as f64).sum();
                assert!((fill[l.0 as usize] - m).abs() < 1e-9, "{label}: {}'s fill {} against the exact {m}", db.def(l.0).name, fill[l.0 as usize]);
            }
        }
    }
    for v in &decks {
        let deck = deck_of(v);
        let kb = list.iter().position(|l| l.0 == b).unwrap();
        let w: f64 = if (0..list.len()).any(|k| v[k] < r[k]) { 0.0 } else { v[kb] as f64 - 1.0 };
        assert!((sighting_weight(&deck, &shown, Some(&seen)) - w).abs() < 1e-12);
        assert_eq!(sighting_weight(&deck, &shown, None), seen_weight(&deck, &shown));
    }
}

/// The truth of a history of sightings by simulation (ENGINE.md 140.9): decks of the prior with nothing shown
/// (`draw`), each shuffled; the events in order - a blind sighting turns over the next card of the shuffled deck, a
/// search finds a card of its definition among the cards not yet turned over or found (the owner finds what it looks
/// for whenever the deck holds one; the rest then reshuffled, as a fetch does) - and the deck kept when every event
/// is as given. Returns the kept decks' histogram of `card`'s copies and the number kept.
fn truth_of_history(draw: &mut dyn FnMut(usize, &mut Rng) -> Vec<SampledDeck>, events: &[(u16, bool)], card: u16, sims: usize, rng: &mut Rng) -> ([f64; 9], usize) {
    let mut hist = [0.0f64; 9];
    let mut kept = 0usize;
    let mut done = 0;
    while done < sims {
        let chunk = (sims - done).min(20_000);
        'deck: for s in draw(chunk, rng) {
            let mut pool = s.deck.clone();
            rng.shuffle(&mut pool);
            for &(d, searched) in events {
                if searched {
                    match pool.iter().position(|&x| x == d) {
                        Some(i) => {
                            pool.swap_remove(i);
                            rng.shuffle(&mut pool);
                        }
                        None => continue 'deck,
                    }
                } else if pool.pop() != Some(d) {
                    continue 'deck;
                }
            }
            kept += 1;
            hist[count(&s.deck, card)] += 1.0;
        }
        done += chunk;
    }
    (hist, kept)
}

/// ENGINE.md 140.9 (the user's model of a search): the owner finds a card it looks for whenever the deck holds one, so
/// a search's find weighs a deck by [D_i > copies seen before], not by the D_i - j of a card seen blind. The Delver 75,
/// Volcanic Island: a fetch land's find alone, a find after one Volcanic seen blind, and one seen blind after a find -
/// the request with its sightings against the truth by simulation, and the identity-blind request (every copy seen
/// blind, ENGINE.md 140's model before this) off by many standard errors in each.
#[test]
fn a_search_finds_what_it_looks_for_whenever_the_deck_holds_one() {
    let db = cdb();
    let (_, _, prior) = delver_prior();
    let volc = db.expect_id("Volcanic Island").unwrap();
    let plain = DeckWant { db, n: 60, shown: &[], companion: None, tilt: None, pick: None, seen: None };
    let mut draw = |n: usize, rng: &mut Rng| prior.sample_k(&plain, &[], n, rng);
    for (label, events, sims) in [
        ("a fetch land finds a Volcanic Island", vec![(volc, true)], 60_000usize),
        ("one seen blind, then a fetch land finds another", vec![(volc, false), (volc, true)], 700_000),
        ("a fetch land finds one, then one is seen blind", vec![(volc, true), (volc, false)], 700_000),
    ] {
        let mut rng = Rng::new(0x5EA4C ^ events.len() as u64 ^ (events[0].1 as u64) << 8);
        let (truth, kept) = truth_of_history(&mut draw, &events, volc, sims, &mut rng);
        let shown = vec![volc; events.len()];
        let searched = events.iter().enumerate().fold(0u64, |m, (j, e)| m | (e.1 as u64) << j);
        let seen = [Seen { def: volc, n: events.len() as u8, searched }];
        let want = DeckWant { db, n: 60, shown: &shown, companion: None, tilt: None, pick: None, seen: Some(&seen) };
        let n = 40_000;
        let req = request_hist(&prior, &want, volc, n, &mut rng);
        assert_same_law(label, &truth, kept, &req, n);
        let fill = prior.expected_fill(&want)[volc as usize] + shown.len() as f64;
        let (mr, sr) = mean_se(&req, n);
        assert!((fill - mr).abs() < 4.5 * sr, "{label}: expected_fill {fill:.4} against the request's {mr:.4} +- {sr:.4}");
        // the identity-blind request (ENGINE.md 140 before 140.9): every copy weighed C(D, R)
        let blind = DeckWant { db, n: 60, shown: &shown, companion: None, tilt: None, pick: None, seen: None };
        let bh = request_hist(&prior, &blind, volc, n, &mut rng);
        let ((mt, st), (mb, sb)) = (mean_se(&truth, kept), mean_se(&bh, n));
        let z = (mb - mt) / (st * st + sb * sb).sqrt();
        println!("   {label}: the identity-blind request {mb:.4} +- {sb:.4}, {z:.1} se from the truth");
        assert!(z.abs() > 8.0, "{label}: the identity-blind request is not told from the truth ({z:.1} se)");
    }
}

/// The table prior (ENGINE.md 140.9) given a search's find: a fill of independent draws is not the posterior of a
/// find (it weighs a deck by [D > j], not D - j), so a found definition's fill comes from the exact joint law first.
/// The EoE table, Forest: a find alone and a find after one seen blind, against the truth by simulation; the
/// identity-blind request (R plus a fresh fill) off.
#[test]
fn the_table_prior_weighs_a_search_by_finding_one() {
    let db = CardDb::eoe();
    let p = TablePrior::of(db);
    let forest = db.id("Forest").unwrap();
    let plain = DeckWant { db, n: 40, shown: &[], companion: None, tilt: None, pick: None, seen: None };
    let mut draw = |n: usize, rng: &mut Rng| (0..n).map(|_| p.sample(&plain, rng)).collect::<Vec<_>>();
    for (label, events, sims) in [("a search finds a Forest", vec![(forest, true)], 60_000usize), ("one seen blind, then a search finds another", vec![(forest, false), (forest, true)], 1_200_000)] {
        let mut rng = Rng::new(0x7AB5E ^ events.len() as u64);
        let (truth, kept) = truth_of_history(&mut draw, &events, forest, sims, &mut rng);
        let shown = vec![forest; events.len()];
        let searched = events.iter().enumerate().fold(0u64, |m, (j, e)| m | (e.1 as u64) << j);
        let seen = [Seen { def: forest, n: events.len() as u8, searched }];
        let want = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None, pick: None, seen: Some(&seen) };
        let n = 40_000;
        let req = request_hist(&*p, &want, forest, n, &mut rng);
        assert_same_law(&format!("table prior, {label}"), &truth, kept, &req, n);
        let fill = p.expected_fill(&want)[forest as usize] + shown.len() as f64;
        let (mr, sr) = mean_se(&req, n);
        assert!((fill - mr).abs() < 4.5 * sr, "{label}: expected_fill {fill:.4} against the request's {mr:.4}");
        let blind = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None, pick: None, seen: None };
        let bh = request_hist(&*p, &blind, forest, n, &mut rng);
        let ((mt, st), (mb, sb)) = (mean_se(&truth, kept), mean_se(&bh, n));
        let z = (mb - mt) / (st * st + sb * sb).sqrt();
        println!("   table prior, {label}: the identity-blind request {mb:.4} +- {sb:.4}, {z:.1} se from the truth");
        assert!(z.abs() > 8.0, "{label}: the identity-blind request is not told from the truth ({z:.1} se)");
    }
}

/// The knowledge layer keeps how each card was seen (ENGINE.md 140.9): a fetch land's find is the owner's search's,
/// a land played from the hand is seen blind, in the order they happened; the owner's own cards are no sightings; a
/// search into the hand that is not revealed is none either.
#[test]
fn the_knowledge_keeps_how_each_card_was_seen() {
    use engine::db::AbilitySlot;
    use engine::testkit::Scenario;
    use engine::{Choice, DecisionKind, Step, Zone};
    let db = cdb();
    let id = |s: &str| db.expect_id(s).unwrap();
    let mut g = Scenario::new(db)
        .spec(|s| {
            s.auto_single = false;
            s.hidden_decks = true;
            s.deck_clouds = true;
        })
        .battlefield(1, &["Scalding Tarn"])
        .hand(1, &["Island"])
        .library(1, &["Volcanic Island", "Brainstorm", "Ponder", "Daze", "Lightning Bolt", "Force of Will"])
        .library(0, &["Island"; 10])
        .at(Step::Main1, 1)
        .start();
    assert!(g.sightings().is_empty());
    let tarn = g.card(1, "Scalding Tarn", Zone::Battlefield);
    g.choose(Choice::Ability(tarn, AbilitySlot::Activated(0)));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary));
    g.choose(Choice::Def(id("Volcanic Island")));
    g.drive_until(|g| g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority));
    let island = g.card(1, "Island", Zone::Hand);
    g.choose(Choice::Play(island));
    let v: Vec<(u8, u8, u16, bool)> = g.sightings().iter().map(|x| (x.viewer, x.owner, x.def, x.searched)).collect();
    assert_eq!(v, vec![(0, 1, id("Volcanic Island"), true), (0, 1, id("Island"), false)], "the find, then the land played");
    assert!(g.sightings()[0].at < g.sightings()[1].at);
}

/// The K-sample deal (ENGINE.md 102.8) draws its K decks of a request with `sample_k` (one table a request, ENGINE.md
/// 140), with or without a face-down pick: every deck meets the request's contract, and the decks are a function of
/// the salt and the request.
#[test]
fn the_k_decks_of_a_request_meet_it() {
    let db = cdb();
    let (_, _, prior) = delver_prior();
    let id = |s: &str| db.expect_id(s).unwrap();
    let mut shown = vec![id("Pyroblast"), id("Lightning Bolt"), id("Lightning Bolt"), id("Volcanic Island")];
    shown.sort_unstable();
    let legal: Vec<u16> = {
        let mut v: Vec<u16> = prior.support().iter().copied().filter(|&d| db.def(d).is(engine::db::types::LAND)).collect();
        v.dedup();
        v
    };
    let pw = PickWant { legal, a: 0.5, b: 1.0 };
    for pick in [None, Some(&pw)] {
        let want = DeckWant { db, n: 60, shown: &shown, companion: None, tilt: None, pick, seen: None };
        let cache = DealCache::new(32, 0x5EED);
        let decks = cache.decks(&prior, 1, &want);
        assert_eq!(decks.len(), 32);
        for dl in decks.iter() {
            let s = SampledDeck { deck: dl.deck.clone(), sideboard: dl.side.to_vec() };
            check_sample(&prior, &want, &s).unwrap();
        }
        // the same request in another cache of the same salt: the same decks
        let again = DealCache::new(32, 0x5EED).decks(&prior, 1, &want);
        assert!(decks.iter().zip(again.iter()).all(|(x, y)| x.deck == y.deck));
    }
}

/// The table prior (ENGINE.md 102.1's stub; FUZZ_HIDDEN_DECKS): its fill is independent draws (lands to round(17 n /
/// 40), spells to n), so a seen card is one of those draws and the rest are independent of it: R plus a fresh fill
/// is already the posterior of what was seen. The truth by looking agrees with the request (on 267ae603 too), and is
/// far from what conditioning on "holds a Forest" would give (E[D | D >= 1], D binomial).
#[test]
fn the_table_prior_is_the_posterior_of_the_cards_the_viewer_saw() {
    let db = CardDb::eoe();
    let p = TablePrior::of(db);
    let forest = db.id("Forest").unwrap();
    let lands = p.support().iter().filter(|&&d| db.def(d).is(engine::db::types::LAND)).count();
    let plain = DeckWant { db, n: 40, shown: &[], companion: None, tilt: None, pick: None, seen: None };
    let mut draw = |n: usize, rng: &mut Rng| (0..n).map(|_| p.sample(&plain, rng)).collect::<Vec<_>>();
    let shown = vec![forest];
    let mut rng = Rng::new(0x7AB1E);
    let (truth, kept) = truth_by_looking(&mut draw, &shown, forest, 600_000, &mut rng);
    let want = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None, pick: None, seen: None };
    let n = 40_000;
    let req = request_hist(&*p, &want, forest, n, &mut rng);
    assert_same_law("table prior, one Forest seen", &truth, kept, &req, n);
    let fill = p.expected_fill(&want)[forest as usize] + 1.0;
    let (mr, sr) = mean_se(&req, n);
    assert!((fill - mr).abs() < 4.5 * sr, "expected_fill {fill:.4} against the request's {mr:.4}");
    // conditioning on holding a Forest would give the other law: Forests ~ Binomial(17, 1 / lands), given >= 1
    let q = 1.0 / lands as f64;
    let held = 17.0 * q / (1.0 - (1.0 - q).powi(17));
    let (mt, st) = mean_se(&truth, kept);
    println!("the EoE table's {lands} supported lands: one Forest seen, {mt:.4} +- {st:.4} by looking, {held:.4} if conditioned on holding one");
    assert!((mt - held).abs() > 6.0 * st, "the test cannot tell the posterior of the sightings from the indicator's");
}

/// The table prior under a revealed Lutri (CR 702.139a; "each nonland card in your starting deck has a different
/// name"): the spells are a uniform subset of the names, the lands independent draws - each group whole, so R plus
/// the fill is still the posterior of the sightings (ENGINE.md 140) and the prior's guard against a group it would
/// only partly limit stays quiet. Every sample keeps one copy per nonland name and holds the seen cards.
#[test]
fn the_table_prior_under_lutri_keeps_its_groups_whole() {
    let cube = CardDb::cube();
    let lutri = cube.id("Lutri, the Spellchaser").unwrap();
    let p = TablePrior::of(cube);
    let spells: Vec<u16> = p.support().iter().copied().filter(|&d| !cube.def(d).is(engine::db::types::LAND) && d != lutri).collect();
    let mut rng = Rng::new(0x1A7);
    for case in 0..200 {
        let mut shown: Vec<u16> = (0..case % 9).map(|i| spells[(case * 37 + i * 101) % spells.len()]).collect();
        shown.sort_unstable();
        shown.dedup();
        let want = DeckWant { db: cube, n: 40, shown: &shown, companion: Some(lutri), tilt: None, pick: None, seen: None };
        let s = p.sample(&want, &mut rng);
        check_sample(&*p, &want, &s).unwrap();
        let mut names: Vec<&str> = s.deck.iter().filter(|&&d| !cube.def(d).is(engine::db::types::LAND)).map(|&d| cube.def(d).name.as_str()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n, "a nonland name twice under Lutri");
        assert_eq!(s.sideboard, vec![lutri]);
    }
}

/// The cost of a new request of the constructed prior (ENGINE.md 140; ignored: a measurement): the K = 32 decks of
/// `DealCache::decks` (a fresh cache each time), against K separate samples (the deal before ENGINE.md 140's one table
/// a request), for no shown card and for a game-2 position's twelve.
#[test]
#[ignore]
fn constructed_request_cost() {
    let db = cdb();
    let (_, _, prior) = delver_prior();
    let id = |s: &str| db.expect_id(s).unwrap();
    let mut twelve: Vec<u16> = ["Volcanic Island", "Volcanic Island", "Polluted Delta", "Lightning Bolt", "Lightning Bolt", "Daze", "Brainstorm", "Pyroblast", "Dragon's Rage Channeler", "Wasteland", "Ponder", "Force of Will"]
        .iter()
        .map(|s| id(s))
        .collect();
    twelve.sort_unstable();
    for (what, sh) in [("no shown card", &[][..]), ("twelve shown cards", &twelve[..])] {
        let want = DeckWant { db, n: 60, shown: sh, companion: None, tilt: None, pick: None, seen: None };
        let reps = 300;
        let t = std::time::Instant::now();
        for i in 0..reps {
            std::hint::black_box(DealCache::new(32, i as u64).decks(&prior, 1, &want));
        }
        let req = t.elapsed().as_secs_f64() * 1e6 / reps as f64;
        let t = std::time::Instant::now();
        let mut rng = Rng::new(1);
        for _ in 0..reps {
            for _ in 0..32 {
                std::hint::black_box(prior.sample(&want, &mut rng));
            }
        }
        let separate = t.elapsed().as_secs_f64() * 1e6 / reps as f64;
        println!("{what}: a new request (32 decks, DealCache::decks) {req:.0} us; 32 separate samples {separate:.0} us");
    }
}
