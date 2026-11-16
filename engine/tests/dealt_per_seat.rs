//! Dealt decks per seat (ENGINE.md 168.1, `"dealt_decks"` with `"per_seat": true`) and sideboarding by a choice table
//! (ENGINE.md 168.3, the spec's `"sideboard_table"`): each seat of a constructed best-of-three is dealt independently
//! with probability p - none, one or both -, both dealt games begin with nobody deciding, the two seats' deals come
//! from streams of their own; a seat of the choice table that is not dealt sideboards to its context's 60 by teacher
//! decisions (zero-core `SearchGame::teacher_policy`), recorded by zero-core's driver as policy targets.

use engine::az::constructed::Decklist;
use engine::az::encode::{g6, mtch, O_GLOBALS_V6, O_MATCH};
use engine::az::matches::{dealt_seat, dealt_seats};
use engine::az::sbtable::{context_index, CONTEXTS};
use engine::az::{MtgAz, MtgSpec};
use engine::db::CardDb;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_DEALT};
use engine::rng::Rng;
use serde_json::json;
use zero_search::driver::{Driver, DriverCfg, GameResult, KeyKind, NnSeat, Record, SeatCfg};
use zero_search::game::{Next, SearchGame, MAX_SEATS};

fn scale(n: u64) -> u64 {
    n * std::env::var("AZ_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

fn delver() -> (Vec<String>, Vec<String>) {
    let d = Decklist::izzet_delver();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    (d.main_names(), side)
}

/// The Delver mirror's post-sideboard matches (the presets', ENGINE.md 130) with the keys of `extra` (null removes).
fn json_of(extra: serde_json::Value) -> String {
    let (main, side) = delver();
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

fn per_seat(p: f64, lo: u8, hi: u8) -> serde_json::Value {
    json!({"p": p, "swaps": [lo, hi], "per_seat": true})
}

/// The Delver 75 as the table's list (first-seen order of main then sideboard), its copies and the main's.
fn list_of() -> (Vec<String>, Vec<u8>, Vec<u8>) {
    let (main, side) = delver();
    let (mut names, mut copies, mut base): (Vec<String>, Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new(), Vec::new());
    for (i, n) in main.iter().chain(&side).enumerate() {
        let k = names.iter().position(|x| x == n).unwrap_or_else(|| {
            names.push(n.clone());
            copies.push(0);
            base.push(0);
            names.len() - 1
        });
        copies[k] += 1;
        if i < main.len() {
            base[k] += 1;
        }
    }
    (names, copies, base)
}

/// A choice table of random legal 60s (the main with random one-for-one swaps), seeded; its choices as given (list
/// order) by context index.
fn table_of(seed: u64) -> (serde_json::Value, Vec<Vec<u8>>) {
    let (names, copies, base) = list_of();
    let mut r = Rng::new(seed);
    let (mut choices, mut by_ctx) = (Vec::new(), vec![Vec::new(); CONTEXTS]);
    for game in 2..=3u8 {
        for first in 0..2u8 {
            for lead in -1..=1i8 {
                for od in 0..2u8 {
                    let mut c = base.clone();
                    for _ in 0..1 + r.below(12) {
                        let out: Vec<usize> = (0..c.len()).filter(|&k| c[k] > 0).collect();
                        let inn: Vec<usize> = (0..c.len()).filter(|&k| c[k] < copies[k]).collect();
                        let (o, i) = (out[r.below(out.len() as u32) as usize], inn[r.below(inn.len() as u32) as usize]);
                        if o != i {
                            c[o] -= 1;
                            c[i] += 1;
                        }
                    }
                    by_ctx[context_index(game, first == 1, lead, od == 1)] = c.clone();
                    choices.push(json!([game, first, lead, od, c]));
                }
            }
        }
    }
    let list: Vec<serde_json::Value> = names.iter().zip(&copies).map(|(n, k)| json!([n, k])).collect();
    (json!({"version": format!("t{seed}"), "list": list, "choices": choices}), by_ctx)
}

/// A context's 60 in the match's list order (the pool's: sorted by definition).
fn in_pool_order(g: &MtgAz, p: usize, given: &[u8]) -> Vec<u8> {
    let (names, _, _) = list_of();
    let db = CardDb::constructed();
    g.m.pools[p].list.iter().map(|&(d, _)| given[names.iter().position(|n| db.id(n) == Some(d)).unwrap()]).collect()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

fn row(g: &MtgAz, viewer: usize) -> Vec<f32> {
    engine::az::tail_encode::Row::of_az(g, viewer).flat()
}

fn next(g: &MtgAz) -> Option<MtgAz> {
    if !g.m.game_episodes {
        return None;
    }
    match g.next_episode(None).unwrap() {
        Next::Continue(n) => Some(n),
        Next::Over(_) => None,
    }
}

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

fn main_ids() -> Vec<u16> {
    let db = CardDb::constructed();
    let mut v: Vec<u16> = delver().0.iter().map(|n| db.id(n).unwrap()).collect();
    v.sort_unstable();
    v
}

#[test]
fn per_seat_and_the_table_parse_and_refuse() {
    let s = spec_of(json!({"dealt_decks": per_seat(0.85, 0, 15)}));
    let dd = s.dealt_decks.unwrap();
    assert!(dd.per_seat && dd.swaps == (0, 15));
    assert!(!spec_of(json!({"dealt_decks": {"p": 0.5, "swaps": [1, 8]}})).dealt_decks.unwrap().per_seat, "the default: one dealt seat");
    let bad = |e: serde_json::Value| MtgSpec::parse(&json_of(e)).unwrap_err();
    assert!(bad(json!({"dealt_decks": {"p": 0.5, "swaps": [1, 8], "per_seat": 1}})).contains("per_seat"));
    let (t, _) = table_of(1);
    let s = spec_of(json!({"sideboard_table": t}));
    assert_eq!(s.sideboard_table.as_ref().unwrap().version, "t1");
    // the evaluation spec drops both (self-play settings)
    let e: serde_json::Value = serde_json::from_str(&MtgSpec::eval_json(&json_of(json!({"sideboard_table": t, "dealt_decks": per_seat(1.0, 0, 15)}))).unwrap()).unwrap();
    assert!(e.get("sideboard_table").is_none() && e.get("dealt_decks").is_none());
    // refusals: not a constructed best-of-three, with keep_deck, a missing context, a context twice, 59 cards, an
    // unknown card, too many copies, an unknown key
    let mut bo1: serde_json::Value = serde_json::from_str(&json_of(json!({"sideboard_table": t, "format": "bo1", "start_game": null, "game_episodes": null}))).unwrap();
    assert!(MtgSpec::parse(&bo1.to_string()).unwrap_err().contains("sideboard_table"));
    bo1["format"] = json!("bo3");
    assert!(bad(json!({"sideboard_table": t, "keep_deck": [0]})).contains("keep_deck"));
    let mut short = t.clone();
    short["choices"].as_array_mut().unwrap().pop();
    assert!(bad(json!({"sideboard_table": short})).contains("contexts"));
    let mut twice = t.clone();
    let c0 = twice["choices"][0].clone();
    twice["choices"][1] = c0;
    assert!(bad(json!({"sideboard_table": twice})).contains("twice"));
    let mut small = t.clone();
    let k = small["choices"][3][4].as_array().unwrap().iter().position(|v| v.as_u64().unwrap() > 0).unwrap();
    let n = small["choices"][3][4][k].as_u64().unwrap();
    small["choices"][3][4][k] = json!(n - 1);
    assert!(bad(json!({"sideboard_table": small})).contains("59 cards"));
    let mut unk = t.clone();
    unk["list"][0][0] = json!("Not A Card");
    assert!(bad(json!({"sideboard_table": unk})).contains("unknown card"));
    let mut many = t.clone();
    many["choices"][0][4][0] = json!(31);
    assert!(bad(json!({"sideboard_table": many})).contains("at most"));
    let mut key = t.clone();
    key["extra"] = json!(1);
    assert!(bad(json!({"sideboard_table": key})).contains("unknown key"));
}

/// Each seat is dealt with probability p on its own (`dealt_seats`, a stream of the match's seed of its own), the
/// match's dealt seats are those (one: `dealt`; both: `dealt_both`), and the per-seat mode shares nothing with the
/// one-seat mode's draw.
#[test]
fn each_seat_is_dealt_independently() {
    let n = 20_000u64;
    let p = 0.85;
    let mut c = [0u64; 4];
    for seed in 0..n {
        let d = dealt_seats(seed, p);
        c[d[0] as usize + 2 * d[1] as usize] += 1;
        assert_eq!(dealt_seats(seed, p), d, "deterministic");
        assert_eq!(dealt_seats(seed, 1.0), [true, true]);
    }
    let want = [(1.0 - p) * (1.0 - p), p * (1.0 - p), (1.0 - p) * p, p * p];
    for i in 0..4 {
        let f = c[i] as f64 / n as f64;
        let se = (want[i] * (1.0 - want[i]) / n as f64).sqrt();
        assert!((f - want[i]).abs() < 5.0 * se, "outcome {i}: {f} against {}", want[i]);
    }
    let s = spec_of(json!({"dealt_decks": per_seat(p, 0, 15)}));
    let (mut both, mut one, mut none) = (0, 0, 0);
    for seed in 0..scale(200) {
        let g = MtgAz::new(&s, seed).unwrap();
        let d = dealt_seats(seed, p);
        assert_eq!([g.m.is_dealt(0), g.m.is_dealt(1)], d, "seed {seed}");
        assert_eq!(g.m.dealt_both, d[0] && d[1]);
        assert_eq!(g.m.dealt, if d[0] ^ d[1] { Some(d[1] as u8) } else { None });
        match d.iter().filter(|&&x| x).count() {
            2 => both += 1,
            1 => one += 1,
            _ => none += 1,
        }
        // (a one-seat spec's draw is its own stream: unchanged by the option)
        let _ = dealt_seat(seed, p);
    }
    assert!(both > 0 && one > 0 && none > 0, "{both} / {one} / {none}");
}

/// Both seats dealt (p 1): the match begins at game 2 with nobody deciding at a sideboarding; each later game's two
/// 60s are legal 60s within the swap range, dealt from streams of their own (they differ), the same from the same
/// seed; game 3 is dealt again. In whole matches the deals are keyed `KEY_DEALT`, each seat's private: a world of the
/// other player keys them `HIDDEN`, the dealt seat's own world in full.
#[test]
fn both_dealt_matches_deal_two_independent_60s_and_key_them_privately() {
    let main = main_ids();
    let s = spec_of(json!({"dealt_decks": per_seat(1.0, 0, 15)}));
    let (mut differ, mut games3, mut n) = (0, 0, 0);
    for seed in 0..scale(60) {
        let mut g = MtgAz::new(&s, seed).unwrap();
        assert!(g.m.dealt_both && g.m.game_no == 2 && g.sideboarding().is_none(), "seed {seed}: game 2 at once");
        let again = MtgAz::new(&s, seed).unwrap();
        assert_eq!((again.m.decks[0].clone(), again.m.decks[1].clone()), (g.m.decks[0].clone(), g.m.decks[1].clone()));
        let (mut rng, mut keys) = (Rng::new(seed ^ 0xB0B), Vec::new());
        let mut seen_game = 0;
        for _ in 0..200_000 {
            if g.m.game_no != seen_game && g.sideboarding().is_none() {
                seen_game = g.m.game_no;
                for q in 0..2u8 {
                    let k = swaps(&main, &g.m.deck(q));
                    assert!(k <= 15 && g.m.deck(q).len() == 60, "seed {seed}: P{q}'s dealt 60 of {k} swaps");
                }
                differ += (g.m.decks[0] != g.m.decks[1]) as u32;
                n += 1;
                games3 += (seen_game == 3) as u32;
            }
            let Some(p) = g.to_move() else {
                match next(&g) {
                    Some(x) => {
                        g = x;
                        continue;
                    }
                    None => break,
                }
            };
            assert!(g.sideboarding().is_none(), "seed {seed}: P{p} decides at a sideboarding of two dealt seats");
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
    }
    assert!(differ as f64 >= 0.95 * n as f64 && games3 > 0, "{differ} of {n} games with two different 60s, {games3} games 3");
    // whole matches: the step that ends game 1 deals both seats, keyed privately
    let whole = spec_of(json!({"dealt_decks": per_seat(1.0, 0, 15), "start_game": null, "game_episodes": null, "max_turns": 12}));
    let mut checked = 0;
    for seed in 0..scale(40) {
        let mut g = MtgAz::new(&whole, seed).unwrap();
        let mut rng = Rng::new(seed ^ 0xD3A1);
        for _ in 0..200_000 {
            let Some(_p) = g.to_move() else { break };
            let l = legal(&g);
            let a = l[rng.below(l.len() as u32) as usize];
            let mut keys = Vec::new();
            let mut probe = g.clone();
            probe.step(a, &mut keys).unwrap();
            let dealt_keys: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_DEALT).collect();
            if !dealt_keys.is_empty() {
                // (the live game: both seats' deals in full)
                for q in 0..2u8 {
                    let mine: Vec<u64> = dealt_keys.iter().copied().filter(|&k| key_player(k) == q).collect();
                    assert!(!mine.is_empty() && mine.iter().all(|&k| key_payload(k) != HIDDEN), "seed {seed}: P{q}'s deal keys");
                }
                // (a world of each viewer, stepped the same: the other seat's deal HIDDEN, its own in full)
                for v in 0..2u8 {
                    let mut w = g.clone();
                    w.determinize(v as usize, &mut zero_search::rng::Rng::new(seed ^ 0x77));
                    let mut wk = Vec::new();
                    if !legal(&w).contains(&a) || w.step(a, &mut wk).is_err() {
                        continue;
                    }
                    for k in wk.iter().copied().filter(|&k| key_kind(k) == KEY_DEALT) {
                        assert_eq!(key_payload(k) == HIDDEN, key_player(k) != v, "seed {seed}: viewer P{v}'s world keys P{}'s deal", key_player(k));
                    }
                    checked += 1;
                }
            }
            g = probe;
        }
    }
    assert!(checked > 0, "no dealing step checked");
}

/// The swap counts of the pb3 preset's dealt 60s ([0, 15]) are uniform over 0..=15 (the constructed prior's base,
/// ENGINE.md 120.2: the user's decision of 2026-11-12), for both seats of both-dealt matches.
#[test]
fn a_dealt_60s_swap_count_is_uniform_over_0_to_15() {
    let main = main_ids();
    let s = spec_of(json!({"dealt_decks": per_seat(1.0, 0, 15)}));
    let mut c = [0u32; 16];
    let n = scale(1600);
    for seed in 0..n {
        let g = MtgAz::new(&s, seed).unwrap();
        for q in 0..2u8 {
            c[swaps(&main, &g.m.deck(q))] += 1;
        }
    }
    let e = 2.0 * n as f64 / 16.0;
    for (k, &x) in c.iter().enumerate() {
        assert!((x as f64 - e).abs() < 5.0 * (e * (15.0 / 16.0)).sqrt(), "{k} swaps: {x} of {e} expected ({c:?})");
    }
}

/// The encoding shows every dealt player (`g6::DEALT` per player, the frame seat first): both floats in a match of two
/// dealt seats, one in a match of one, none without a dealt seat.
#[test]
fn rows_show_every_dealt_player() {
    let s = spec_of(json!({"dealt_decks": per_seat(0.6, 0, 15)}));
    let mut seen = [0u32; 3];
    for seed in 0..scale(30) {
        let g = MtgAz::new(&s, seed).unwrap();
        let Some(p) = g.to_move() else { continue };
        for v in [p, 1 - p] {
            let x = row(&g, v);
            let frame = if g.sideboarding().is_some() { v as u8 } else { g.g.encoding_seat(v as u8) };
            let f = [x[O_GLOBALS_V6 + g6::DEALT], x[O_GLOBALS_V6 + g6::PLAYER + g6::DEALT]];
            assert_eq!(f, [g.m.is_dealt(frame) as u8 as f32, g.m.is_dealt(1 - frame) as u8 as f32], "seed {seed}: P{v}'s dealt floats");
        }
        seen[g.m.is_dealt(0) as usize + g.m.is_dealt(1) as usize] += 1;
    }
    assert!(seen.iter().all(|&k| k > 0), "{seen:?}");
}

/// A seat of the choice table that is not dealt: at each of its sideboarding decisions the teacher policy is one-hot on
/// a legal action (and nothing at any other decision, a dealt seat's included); following it, the next game's 60 is
/// exactly its context's - the next game, first or second, its lead, the other seat dealt or not.
#[test]
fn a_table_seat_sideboards_to_its_contexts_60() {
    let (t, by_ctx) = table_of(7);
    let s = spec_of(json!({"sideboard_table": t, "dealt_decks": per_seat(0.5, 0, 15)}));
    let whole = spec_of(json!({"sideboard_table": t, "dealt_decks": per_seat(0.5, 0, 15), "start_game": null, "game_episodes": null, "max_turns": 12}));
    let (mut reached, mut taught, mut ctxs) = (0, 0, std::collections::BTreeSet::new());
    for (sp, label) in [(&s, "post"), (&whole, "whole")] {
        for seed in 0..scale(60) {
            let mut g = MtgAz::new(sp, seed).unwrap();
            let mut rng = Rng::new(seed ^ 0x7EAC);
            let mut target: [Option<Vec<u8>>; 2] = [None, None];
            let mut game = g.m.game_no;
            for _ in 0..200_000 {
                if g.m.game_no != game && g.sideboarding().is_none() {
                    game = g.m.game_no;
                    for q in 0..2 {
                        if let Some(t) = target[q].take() {
                            assert_eq!(&g.m.decks[q][..], &t[..], "{label} seed {seed}: P{q}'s game {game} 60 is not its context's");
                            reached += 1;
                        }
                    }
                }
                let Some(p) = g.to_move() else {
                    match next(&g) {
                        Some(x) => {
                            g = x;
                            continue;
                        }
                        None => break,
                    }
                };
                let l = legal(&g);
                let mut pi = Vec::new();
                let teach = g.teacher_policy(&l, &mut pi);
                let sb = g.sideboarding().is_some();
                assert_eq!(teach, sb, "{label} seed {seed}: a teacher decision at a sideboarding only (both seats are the table's)");
                assert!(!(sb && g.m.is_dealt(p as u8)));
                let a = if teach {
                    assert_eq!(pi.len(), l.len());
                    assert_eq!(pi.iter().filter(|&&w| w == 1.0).count(), 1, "one-hot");
                    assert!(pi.iter().all(|&w| w == 0.0 || w == 1.0));
                    let s = g.sideboarding().unwrap();
                    let lead = g.m.wins[p] as i8 - g.m.wins[1 - p] as i8;
                    let k = context_index(g.m.game_no + 1, s.turn == 0, lead, g.m.is_dealt(1 - p as u8));
                    ctxs.insert(k);
                    target[p] = Some(in_pool_order(&g, p, &by_ctx[k]));
                    taught += 1;
                    l[pi.iter().position(|&w| w == 1.0).unwrap()]
                } else {
                    l[rng.below(l.len() as u32) as usize]
                };
                g.step(a, &mut Vec::new()).unwrap();
            }
        }
    }
    println!("{taught} teacher decisions, {reached} table 60s reached, {} contexts met", ctxs.len());
    assert!(reached > 20 && ctxs.len() >= 6, "{reached} table 60s reached over {} contexts", ctxs.len());
}

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

/// zero-core's self-play driver (a uniform evaluator, 4 simulations, recording, the aux targets): each match's row,
/// its samples, and the driver's teacher decision count.
pub fn self_play(json: &str, matches: usize, seed: u64) -> (Vec<(GameResult, Vec<Record>)>, u64) {
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
        assert_eq!(a, smp.len());
    };
    while let Some(b) = d.next_batch(true).unwrap() {
        answer(&mut d, &b);
        take(&mut d, &mut rows);
    }
    take(&mut d, &mut rows);
    rows.sort_by_key(|(r, _)| r.gid);
    (rows, d.stats().teacher_decisions)
}

/// The driver plays a table seat's sideboarding by the table: every recorded sideboarding row is a teacher row - its
/// policy target one-hot on a legal action, its q the outcome -, of a seat that is not dealt (its own dealt float 0);
/// the driver counts the teacher decisions; without the table the sideboarding rows are searched (no teacher decision).
#[test]
fn self_play_records_the_tables_choice_as_the_policy_target() {
    let (t, _) = table_of(11);
    let (rows, taught) = self_play(&json_of(json!({"sideboard_table": t, "dealt_decks": per_seat(0.5, 0, 15)})), 16, 168);
    let (mut sb_rows, mut game_rows) = (0, 0);
    for (r, smp) in &rows {
        assert!(r.over, "match {}", r.gid);
        for s in smp {
            if s.opts.is_empty() {
                continue;
            }
            if s.x[O_MATCH + mtch::SIDEBOARDING] > 0.5 {
                assert_eq!(s.x[O_GLOBALS_V6 + g6::DEALT], 0.0, "match {}: a sideboarding row of a dealt seat", r.gid);
                assert_eq!(s.pi.iter().filter(|&&w| w == 1.0).count(), 1, "match {}: a one-hot target {:?}", r.gid, s.pi);
                assert!(s.pi.iter().all(|&w| w == 0.0 || w == 1.0));
                assert_eq!(s.q, s.z, "a teacher row's q is the outcome");
                sb_rows += 1;
            } else {
                game_rows += 1;
            }
        }
    }
    println!("{sb_rows} teacher rows, {game_rows} searched rows; the driver's teacher decisions {taught}");
    assert!(sb_rows > 0 && game_rows > 0 && taught as usize >= sb_rows, "{sb_rows} teacher rows of {taught} teacher decisions");
    let (_, none) = self_play(&json_of(json!({"dealt_decks": per_seat(0.5, 0, 15)})), 6, 168);
    assert_eq!(none, 0, "no table, no teacher decision");
}

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

/// One match hashed: per decision the decider, kind, actions, keys, every 10th the whole row of the decider, every 20th
/// a world of the decider (its decks, a sideboarding's chosen decks, its row); a table seat plays the teacher's action.
fn digest_match(sp: &MtgSpec, seed: u64) -> u64 {
    let mut g = MtgAz::new(sp, seed).unwrap();
    let (mut rng, mut keys) = (Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 168), Vec::new());
    let mut h = H::new();
    h.u64(g.m.is_dealt(0) as u64 | (g.m.is_dealt(1) as u64) << 1);
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
        let a = g.table_action().unwrap_or_else(|| l[rng.below(l.len() as u32) as usize]);
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

/// `cargo test --release -p engine --test misc_suite dealt_per_seat:: -- --ignored --nocapture`: 40 post-sideboard matches of the pb3
/// preset's self-play spec (per seat p 0.85, swaps [0, 15]) with a choice table and 20 whole matches (game 1 played, p
/// 0.5) hashed against `tests/data/digests_dealt_per_seat.txt`; `DIGEST_WRITE=1` records it.
#[test]
#[ignore]
fn dealt_per_seat_digests() {
    let (t, _) = table_of(168);
    let post = spec_of(json!({"dealt_decks": per_seat(0.85, 0, 15), "sideboard_table": t}));
    let whole = spec_of(json!({"dealt_decks": per_seat(0.5, 0, 15), "sideboard_table": t, "start_game": null, "game_episodes": null, "max_turns": 30}));
    let mut lines = Vec::new();
    for seed in 0..40 {
        lines.push(format!("p {seed} {:016x}", digest_match(&post, seed)));
    }
    for seed in 0..20 {
        lines.push(format!("w {seed} {:016x}", digest_match(&whole, seed)));
    }
    let mut all = H::new();
    for l in &lines {
        for b in l.bytes() {
            all.u64(b as u64);
        }
    }
    println!("dealt_per_seat_digests: {} matches; combined digest {:016x}", lines.len(), all.0);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/digests_dealt_per_seat.txt");
    if std::env::var("DIGEST_WRITE").is_ok() {
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        println!("wrote {}", path.display());
        return;
    }
    let recorded = std::fs::read_to_string(&path).expect("tests/data/digests_dealt_per_seat.txt (record with DIGEST_WRITE=1)");
    let rec: Vec<&str> = recorded.lines().collect();
    assert_eq!(rec.len(), lines.len());
    let differ: Vec<&String> = lines.iter().zip(&rec).filter(|(a, b)| a.as_str() != **b).map(|(a, _)| a).collect();
    assert!(differ.is_empty(), "{} of {} matches differ; first: {:?}", differ.len(), lines.len(), &differ[..differ.len().min(5)]);
}
