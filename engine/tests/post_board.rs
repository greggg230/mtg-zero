//! Post-sideboard games only (ENGINE.md 130): a constructed best-of-three that starts at the sideboarding
//! before game 2 (`"start_game": 2`, with one episode per game), game 1's result drawn from the match's seed;
//! a real game 2 encodes exactly as a synthetic one (`OPP_REVEALED` counts the post-sideboard games' reveals
//! only, game 1's chooser and starter are not encoded); and the yardstick's seat that keeps its deck
//! (`"keep_deck"`), which changes nothing the other seat knows or deals.

use engine::az::bots::Heuristic;
use engine::az::constructed::Decklist;
use engine::az::encode::{bag, mtch, NUM_FEATURES, O_BAGS, O_MATCH};
use engine::az::identity::D;
use engine::az::matches::game_one_winner;
use engine::az::{MtgAz, MtgSpec};
use engine::decision::act;
use engine::game::EndReason;
use engine::rng::Rng;
use engine::DecisionKind;
use zero_search::driver::{Driver, DriverCfg, GameResult, KeyKind, NnSeat, Record, SeatCfg};
use zero_search::game::{Next, SearchGame, MAX_SEATS};

fn scale(n: u64) -> u64 {
    n * std::env::var("AZ_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1)
}

/// The Delver mirror's constructed best-of-three (both seats the pinned 75, ENGINE.md 120) with one episode per
/// game (ENGINE.md 121) and the keys of `extra` (JSON).
fn json_of(extra: serde_json::Value) -> String {
    let d = Decklist::izzet_delver();
    let main = d.main_names();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    let mut v = serde_json::json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true,
        "decks": {"p0": main, "p1": main, "p0_sideboard": side, "p1_sideboard": side}});
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v.to_string()
}

fn spec_of(extra: serde_json::Value) -> MtgSpec {
    MtgSpec::parse(&json_of(extra)).unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// The encoding of `viewer`, bit for bit: its whole row (layout v7, ENGINE.md 125.12 - the head, its first
/// [`NUM_FEATURES`] floats, then the tail tokens, a later game's `my_revealed` among them, and the pointer
/// overrides).
fn enc(g: &MtgAz, viewer: usize) -> Vec<u32> {
    let x = engine::az::tail_encode::Row::of_az(g, viewer).flat();
    assert!(x.len() > NUM_FEATURES);
    x.iter().map(|f| f.to_bits()).collect()
}

/// The state after a game's end (one episode per game): the next episode's.
fn next(g: &MtgAz) -> Option<MtgAz> {
    match g.next_episode(None).unwrap() {
        Next::Continue(n) => Some(n),
        Next::Over(_) => None,
    }
}

/// Half the heuristic's choice, half a random one (mana abilities with priority one pick in eight, as the
/// public-state gate's play: uniform picks float most of the mana).
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

fn revealed(g: &MtgAz, p: usize) -> usize {
    g.m.earlier[p].iter().map(|w| w.count_ones() as usize).sum()
}

#[test]
fn start_game_and_keep_deck_are_constructed_best_of_three_settings() {
    let s = spec_of(serde_json::json!({"start_game": 2}));
    assert_eq!(s.start_game, 2);
    assert_eq!(spec_of(serde_json::json!({})).start_game, 1);
    // refused: outside a constructed best-of-three, without one episode per game, with game 1's chooser, game 3
    let err = |extra: serde_json::Value, drop: &[&str]| {
        let mut v: serde_json::Value = serde_json::from_str(&json_of(extra)).unwrap();
        for k in drop {
            v.as_object_mut().unwrap().remove(*k);
        }
        MtgSpec::parse(&v.to_string()).unwrap_err()
    };
    assert!(err(serde_json::json!({"start_game": 2}), &["game_episodes"]).contains("needs game_episodes"));
    assert!(err(serde_json::json!({"start_game": 2, "chooser": 0}), &[]).contains("chooser is game 1's"));
    assert!(err(serde_json::json!({"start_game": 3}), &[]).contains("1 or 2"));
    assert!(err(serde_json::json!({"start_game": 0}), &[]).contains("1 or 2"));
    let e = MtgSpec::parse(r#"{"format": "bo3", "game_episodes": true, "start_game": 2}"#).unwrap_err();
    assert!(e.contains("constructed best-of-three"), "{e}");
    let d = Decklist::izzet_delver();
    let bo1 = serde_json::json!({"constructed": true, "decks": {"p0": d.main_names(), "p1": d.main_names()}, "start_game": 2});
    assert!(MtgSpec::parse(&bo1.to_string()).unwrap_err().contains("constructed best-of-three"));
    // keep_deck: seats, a constructed best-of-three
    let k = spec_of(serde_json::json!({"keep_deck": [1]}));
    assert_eq!(k.keep_deck, [false, true]);
    assert_eq!(spec_of(serde_json::json!({"keep_deck": [0, 1]})).keep_deck, [true, true]);
    assert!(err(serde_json::json!({"keep_deck": [2]}), &[]).contains("seats"));
    assert!(err(serde_json::json!({"keep_deck": [1, 1]}), &[]).contains("twice"));
    assert!(MtgSpec::parse(r#"{"format": "bo3", "keep_deck": [0]}"#).unwrap_err().contains("constructed best-of-three"));
    // the evaluation spec keeps both (it changes only random decks)
    let e = MtgSpec::parse(&MtgSpec::eval_json(&json_of(serde_json::json!({"start_game": 2, "keep_deck": [0]}))).unwrap()).unwrap();
    assert_eq!((e.start_game, e.keep_deck), (2, [true, false]));
}

/// The match starts at the sideboarding before game 2: game 1's result drawn (each seat 1/2, no draw), its
/// loser sideboarding first with the registered main, then choosing play or draw for game 2 (CR 103.1); game 3
/// follows exactly when the game-1 loser wins game 2. Deterministic from the seed.
#[test]
fn a_match_starts_at_the_sideboarding_before_game_2() {
    let s = spec_of(serde_json::json!({"start_game": 2}));
    let full = spec_of(serde_json::json!({}));
    let n = scale(400);
    let mut wins0 = 0u64;
    for seed in 0..n {
        let g = MtgAz::new(&s, seed).unwrap();
        let fresh = MtgAz::new(&full, seed).unwrap();
        let w = game_one_winner(seed);
        wins0 += (w == 0) as u64;
        let l = 1 - w;
        assert!(g.m.post_board && g.chained());
        assert_eq!((g.m.game_no, g.m.results, g.m.wins, g.m.draws), (1, [1 + w, 0, 0], if w == 0 { [1, 0] } else { [0, 1] }, 0));
        assert_eq!(g.g.outcome().map(|o| (o.winner, o.reason)), Some((Some(w), EndReason::Conceded)), "game 1: the drawn loser concedes it");
        let sb = g.sideboarding().expect("the sideboarding before game 2");
        assert_eq!((sb.decider(), g.to_move()), (l, Some(l as usize)), "seed {seed}: game 1's loser sideboards first (CR 103.1)");
        // both decks the registered mains (a match's game-1 decks), the loser's sideboarding from its main
        assert_eq!(g.m.decks, fresh.m.decks);
        assert_eq!(sb.counts, fresh.m.decks[l as usize].to_vec());
        assert!(legal(&g).contains(&act::PASS));
        assert_eq!(revealed(&g, 0) + revealed(&g, 1), 0);
        assert!(!g.can_concede() && !g.aborted() && g.result()[..2] == [0.5, 0.5]);
        // the same seed, the same state
        let h = MtgAz::new(&s, seed).unwrap();
        assert_eq!((enc(&g, 0), enc(&g, 1), legal(&g)), (enc(&h, 0), enc(&h, 1), legal(&h)));
    }
    // each seat wins game 1 with probability 1/2 (within 4 standard errors)
    let (p, se) = (wins0 as f64 / n as f64, (0.25 / n as f64).sqrt());
    println!("game 1 won by P0 in {wins0} of {n} matches");
    assert!((p - 0.5).abs() < 4.0 * se, "game 1 won by P0 in {wins0} of {n}");
    // played on (random actions): game 2's first decision is the game-1 loser's play or draw; a third game iff the
    // loser won game 2
    let (mut games, mut threes) = (0, 0);
    for seed in 0..scale(24) {
        let mut g = MtgAz::new(&s, seed).unwrap();
        let w = game_one_winner(seed);
        let mut rng = Rng::new(seed ^ 0x9A3E);
        let mut keys = Vec::new();
        let mut first = None;
        loop {
            if g.to_move().is_none() {
                match next(&g) {
                    Some(x) => {
                        g = x;
                        continue;
                    }
                    None => break,
                }
            }
            if g.m.game_no == 2 && g.sideboarding().is_none() && first.is_none() {
                first = Some((g.g.kind(), g.g.turn().chooser));
            }
            let l = legal(&g);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
        assert_eq!(first, Some((Some(DecisionKind::PlayDraw), 1 - w)), "seed {seed}: game 2 begins with the loser's choice");
        if g.match_aborted() {
            continue;
        }
        games += g.m.game_no as u64 - 1;
        let r2 = g.m.results[1];
        assert_eq!(g.m.game_no == 3, r2 == 2 - w || r2 == 3, "seed {seed}: game 3 iff game 1's loser won game 2 (or it was drawn): {:?}", g.m.results);
        threes += (g.m.game_no == 3) as u64;
    }
    assert!(threes > 0 && games > scale(24), "{games} games played, {threes} matches of three");
}

/// Two states at the same point of game 2 (or 3, or the sideboarding before it): the same decider, legal
/// actions and encodings of both viewers, bit for bit; in a game the same `Game`; and every few steps a world of
/// each viewer dealt with the same generator - the same decider's actions, encodings, the opponent's dealt 60 and
/// sideboarding.
fn same_state(a: &MtgAz, b: &MtgAz, seed: u64, step: usize, worlds: bool, n: &mut u64) {
    let at = || format!("seed {seed} step {step} (game {}, sideboarding {})", a.m.game_no, a.sideboarding().is_some());
    assert_eq!(a.to_move(), b.to_move(), "{}", at());
    assert_eq!(legal(a), legal(b), "{}: legal actions", at());
    assert_eq!((a.m.game_no, a.m.results, a.m.wins, a.m.draws), (b.m.game_no, b.m.results, b.m.wins, b.m.draws), "{}", at());
    for v in 0..2 {
        let (x, y) = (enc(a, v), enc(b, v));
        if let Some(i) = (0..x.len()).find(|&i| x[i] != y[i]) {
            panic!("{}: P{v}'s encodings differ at {i} ({} / {})", at(), f32::from_bits(x[i]), f32::from_bits(y[i]));
        }
    }
    if a.sideboarding().is_none() {
        assert_eq!(a.g.fingerprint(), b.g.fingerprint(), "{}: the game", at());
    }
    if !worlds {
        return;
    }
    for v in 0..2 {
        let s = seed ^ (step as u64) << 24 ^ v as u64;
        let (mut x, mut y) = (a.clone(), b.clone());
        x.determinize(v, &mut zero_search::rng::Rng::new(s));
        y.determinize(v, &mut zero_search::rng::Rng::new(s));
        assert_eq!(legal(&x), legal(&y), "{}: P{v}'s worlds' actions", at());
        assert_eq!(enc(&x, v), enc(&y, v), "{}: P{v}'s worlds' encodings", at());
        assert_eq!((&x.m.decks, &x.m.pools[1 - v].list), (&y.m.decks, &y.m.pools[1 - v].list), "{}: P{v}'s worlds' decks", at());
        assert_eq!(x.sideboarding().map(|s| (s.done.clone(), s.counts.clone())), y.sideboarding().map(|s| (s.done.clone(), s.counts.clone())));
        if x.sideboarding().is_none() {
            assert_eq!(x.g.fingerprint(), y.g.fingerprint(), "{}: P{v}'s worlds", at());
        }
        *n += 1;
    }
}

/// A real game 2 encodes exactly as a synthetic one (ENGINE.md 130; the user's requirement, for MTGO, where game 1
/// is played): a match whose game 1 is played (one episode per game) and the synthetic start of the same seed
/// with the same game-1 result (`skip_game_one`) - for every viewer at the sideboarding before game 2 and through
/// game 2 (and its game 3), the same legal actions, the same encodings bit for bit, the same `Game`, and worlds
/// dealt alike; game 1's reveals are not in `OPP_REVEALED` while game 2's are in game 3's. Where the seed's own
/// draw gives that result, the spec's `"start_game": 2` is that very state.
#[test]
fn a_real_game_2_encodes_as_a_synthetic_one() {
    let full = spec_of(serde_json::json!({}));
    let post = spec_of(serde_json::json!({"start_game": 2}));
    let (mut matches, mut drawn, mut steps, mut worlds, mut threes, mut own) = (0u64, 0u64, 0usize, 0u64, 0u64, 0u64);
    let (mut g1_reveals, mut g3_reveals) = (0u64, 0u64);
    for seed in 0..scale(40) {
        let mut real = MtgAz::new(&full, seed).unwrap();
        let (mut rng, mut h, mut keys) = (Rng::new(seed ^ 0xB0A2D), Heuristic::new(seed), Vec::new());
        while real.to_move().is_some() {
            let a = mixed(&real, &mut h, &mut rng);
            real.step(a, &mut keys).unwrap();
        }
        if real.match_aborted() || real.m.results[0] == 3 {
            // (a drawn game 1 has no synthetic twin: the draw is never drawn)
            drawn += 1;
            continue;
        }
        let w = real.m.results[0] - 1;
        // game 1 revealed cards, and none of them is kept for the encoding (ENGINE.md 130)
        g1_reveals += real.m.seen.iter().any(|s| s.iter().any(|&b| b != 0)) as u64;
        assert_eq!(revealed(&real, 0) + revealed(&real, 1), 0, "seed {seed}: game 1's reveals kept");
        let mut real = next(&real).expect("one game played");
        let mut syn = MtgAz::new(&full, seed).unwrap();
        syn.skip_game_one(w).unwrap();
        if game_one_winner(seed) == w {
            let s2 = MtgAz::new(&post, seed).unwrap();
            same_state(&s2, &syn, seed, 0, true, &mut worlds);
            own += 1;
        }
        // game 1's chooser and starter are not encoded
        for v in 0..2 {
            let x = enc(&real, v);
            for f in [mtch::STARTED, mtch::CHOSE] {
                assert_eq!(x[O_MATCH + f], 0, "seed {seed}: game 1's {f} encoded");
            }
        }
        // in lockstep: the real match's actions in both
        let mut k = 0usize;
        loop {
            match (real.to_move(), syn.to_move()) {
                (None, None) => {
                    // the game's end: the same game, result and score
                    assert_eq!((real.g.fingerprint(), real.m.results, real.m.wins), (syn.g.fingerprint(), syn.m.results, syn.m.wins), "seed {seed}");
                    assert_eq!((real.result(), real.margin(0)), (syn.result(), syn.margin(0)));
                    match (next(&real), next(&syn)) {
                        (Some(a), Some(b)) => {
                            // the sideboarding before game 3: game 2's reveals count (OPP_REVEALED)
                            threes += 1;
                            g3_reveals += (revealed(&a, 0) + revealed(&a, 1) > 0) as u64;
                            let x = enc(&a, 0);
                            let n = (0..D).filter(|&i| x[O_BAGS + bag::OPP_REVEALED * D + i] != 0).count();
                            assert_eq!(n, revealed(&a, 1), "seed {seed}: OPP_REVEALED is game 2's reveals");
                            real = a;
                            syn = b;
                            continue;
                        }
                        (None, None) => break,
                        _ => panic!("seed {seed}: one match over, the other not"),
                    }
                }
                (a, b) => assert_eq!(a, b, "seed {seed}"),
            }
            same_state(&real, &syn, seed, k, k % 7 == 0, &mut worlds);
            let a = mixed(&real, &mut h, &mut rng);
            let (mut k1, mut k2) = (Vec::new(), Vec::new());
            real.step(a, &mut k1).unwrap();
            syn.step(a, &mut k2).unwrap();
            assert_eq!(k1, k2, "seed {seed} step {k}: chance keys");
            k += 1;
        }
        steps += k;
        matches += 1;
    }
    println!(
        "{matches} matches compared ({drawn} with game 1 drawn or aborted skipped), {steps} steps, {worlds} worlds, {threes} third games, \
         game 1 revealed cards in {g1_reveals}, game 2 in {g3_reveals} of the third games; {own} starts of the spec itself"
    );
    assert!(matches >= scale(40) * 3 / 4 && own > 0 && threes > 0 && g1_reveals > matches / 2 && g3_reveals > 0);
}

/// The yardstick's seat that keeps its deck (`"keep_deck"`, ENGINE.md 130): `PASS` its only sideboarding option,
/// and nothing changes for the other seat - two matches of the same seed, one with seat k keeping, one without
/// (where k passes at its first sideboarding decision too), give the other seat the same encodings and legal
/// actions at every step, and its worlds (dealt with the same generator, the keeper's 60 by `ConstructedPrior`)
/// the same decks, encodings and actions, the keeper's sideboarding in them its full options; the keeper's own
/// worlds keep it keeping.
#[test]
fn a_seat_that_keeps_its_deck_changes_nothing_the_other_seat_knows() {
    let (mut matches, mut keeper_decisions, mut worlds, mut full_in_worlds) = (0, 0, 0u64, 0u64);
    for seed in 0..scale(24) {
        let k = (seed % 2) as usize;
        let o = 1 - k;
        let start = if seed % 3 == 0 { 1 } else { 2 };
        let keep = spec_of(serde_json::json!({"start_game": start, "keep_deck": [k]}));
        let plain = spec_of(serde_json::json!({"start_game": start}));
        let (mut a, mut b) = (MtgAz::new(&keep, seed).unwrap(), MtgAz::new(&plain, seed).unwrap());
        let (mut rng, mut h, mut step) = (Rng::new(seed ^ 0x4EE9), Heuristic::new(seed), 0usize);
        loop {
            if a.to_move().is_none() {
                assert!(b.to_move().is_none());
                match (next(&a), next(&b)) {
                    (Some(x), Some(y)) => {
                        a = x;
                        b = y;
                        continue;
                    }
                    (None, None) => break,
                    _ => panic!("seed {seed}"),
                }
            }
            let p = a.to_move().unwrap();
            assert_eq!(b.to_move(), Some(p));
            let (la, lb) = (legal(&a), legal(&b));
            let keeper_sb = p == k && a.sideboarding().is_some();
            if keeper_sb {
                assert_eq!(la, vec![act::PASS], "seed {seed}: the keeper's only option");
                assert!(lb.len() > 1 && lb.contains(&act::PASS));
                keeper_decisions += 1;
            } else {
                assert_eq!(la, lb, "seed {seed} step {step}");
            }
            assert_eq!(enc(&a, o), enc(&b, o), "seed {seed} step {step}: the other seat's encoding");
            if step % 5 == 0 || a.sideboarding().is_some() {
                for v in [o, k] {
                    let s = seed ^ (step as u64) << 20 ^ v as u64;
                    let (mut x, mut y) = (a.clone(), b.clone());
                    x.determinize(v, &mut zero_search::rng::Rng::new(s));
                    y.determinize(v, &mut zero_search::rng::Rng::new(s));
                    if v == o {
                        assert_eq!(enc(&x, o), enc(&y, o), "seed {seed} step {step}: the other seat's worlds' encodings");
                        assert_eq!(legal(&x), legal(&y), "seed {seed} step {step}: the other seat's worlds' actions");
                        assert_eq!((&x.m.decks, &x.m.pools), (&y.m.decks, &y.m.pools), "seed {seed}: the keeper's 60 dealt alike");
                        assert!(!x.m.keep[k] && x.sideboarding().is_none_or(|s| !s.keep[k]));
                        if x.sideboarding().is_none() {
                            assert_eq!(x.g.fingerprint(), y.g.fingerprint());
                        }
                        // the world plays on alike: the keeper's sideboarding in it is any player's
                        let mut kx = Vec::new();
                        for _ in 0..12 {
                            if x.to_move().is_none() {
                                break;
                            }
                            let l = legal(&x);
                            assert_eq!(l, legal(&y), "seed {seed} step {step}: a world of the other seat played on");
                            full_in_worlds += (x.to_move() == Some(k) && x.sideboarding().is_some() && l.len() > 1) as u64;
                            let c = l[rng.below(l.len() as u32) as usize];
                            let mut ky = Vec::new();
                            kx.clear();
                            x.step(c, &mut kx).unwrap();
                            y.step(c, &mut ky).unwrap();
                            assert_eq!(kx, ky);
                            assert_eq!(enc(&x, o), enc(&y, o), "seed {seed} step {step}: a world of the other seat played on");
                        }
                        worlds += 1;
                    } else if keeper_sb {
                        assert_eq!(legal(&x), vec![act::PASS], "seed {seed}: the keeper's own worlds keep it keeping");
                    }
                }
            }
            let c = if keeper_sb { act::PASS } else { mixed(&a, &mut h, &mut rng) };
            let (mut k1, mut k2) = (Vec::new(), Vec::new());
            a.step(c, &mut k1).unwrap();
            b.step(c, &mut k2).unwrap();
            assert_eq!(k1, k2);
            step += 1;
        }
        assert_eq!(a.m.results, b.m.results);
        matches += 1;
    }
    println!("{matches} twin matches, {keeper_decisions} keeper decisions, {worlds} worlds of the other seat, {full_in_worlds} keeper sideboarding decisions with every option in them");
    assert!(keeper_decisions >= matches && full_in_worlds > 0);
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

/// Self-play matches through zero-core's driver (a uniform evaluator, 4 simulations, recording): each match's
/// result row and its samples.
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
        aux: false,
        paired: false,
        sparse: false,
        branch: None,
        starts: None,
        // (layout v7's row limit, the game's own: ENGINE.md 125)
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

/// Every recorded row is of game 2 or 3 (the match block's game number, the sideboarding before a game among its
/// rows) and labelled by its own game's outcome; a match is one or two episodes (games 2 and 3), its result by
/// game wins with game 1's drawn result counted.
#[test]
fn every_row_is_a_post_sideboard_games_with_its_own_label() {
    let rows = self_play(&json_of(serde_json::json!({"start_game": 2})), 12, 41);
    assert_eq!(rows.len(), 12);
    let (mut n, mut by_game, mut sb_rows) = (0, [0usize; 4], 0);
    for (r, smp) in &rows {
        assert!(r.over && (1..=2).contains(&r.episodes.len()), "match {}: {} episodes", r.gid, r.episodes.len());
        let mut a = 0;
        for (e_i, e) in r.episodes.iter().enumerate() {
            let game = e_i + 2;
            for s in &smp[a..a + e.samples as usize] {
                let sb = s.x[O_MATCH + mtch::SIDEBOARDING] > 0.5;
                let g = (s.x[O_MATCH + mtch::GAME] * 3.0).round() as usize + sb as usize;
                assert_eq!(g, game, "match {}: a row of game {g} among game {game}'s", r.gid);
                for j in 0..2 {
                    let seat = (s.player as usize + j) % 2;
                    assert_eq!(s.z[j], e.result[seat], "match {} game {game}: z is its game's", r.gid);
                }
                by_game[g] += 1;
                sb_rows += sb as usize;
                n += 1;
            }
            a += e.samples as usize;
        }
        assert_eq!(a, smp.len());
        // the match's result: by game wins, game 1's drawn one counted - one episode: game 2 won by game 1's
        // winner, who wins the match; two: game 3's winner wins it, or 1-1 and a drawn game 3 is a drawn match
        let winner = |e: usize| (0..2).find(|&p| r.episodes[e].result[p] > 0.75);
        match (r.episodes.len(), winner(0)) {
            (1, w) => assert_eq!(r.result[w.expect("game 2 won: no game 3")], 1.0, "match {}", r.gid),
            // (game 2 won by game 1's loser: 1-1, game 3 decides; a drawn game 2 is not checked here)
            (_, w2) => match (winner(1), w2) {
                (Some(w3), Some(_)) => assert_eq!(r.result[w3], 1.0, "match {}", r.gid),
                (None, Some(_)) => assert_eq!(r.result[..2], [0.5, 0.5], "match {}", r.gid),
                _ => {}
            },
        }
    }
    println!("{n} rows: game 1 {}, game 2 {}, game 3 {}; {sb_rows} sideboarding rows", by_game[1], by_game[2], by_game[3]);
    assert!(by_game[1] == 0 && by_game[2] > 0 && sb_rows > 0);
}

/// A post-sideboard match replays exactly from its seed and actions (with `next_episode` at each game's end).
#[test]
fn a_post_sideboard_match_replays_from_its_seed_and_actions() {
    let s = spec_of(serde_json::json!({"start_game": 2, "keep_deck": [1]}));
    for seed in 0..scale(6) {
        let play = |acts: Option<&[u16]>| -> (Vec<u16>, Vec<u64>, u64, [u8; 3]) {
            let mut g = MtgAz::new(&s, seed).unwrap();
            let mut rng = Rng::new(seed ^ 0x2E9);
            let (mut out, mut keys) = (Vec::new(), Vec::new());
            loop {
                if g.to_move().is_none() {
                    match next(&g) {
                        Some(x) => {
                            g = x;
                            continue;
                        }
                        None => break,
                    }
                }
                let l = legal(&g);
                let a = match acts {
                    Some(v) => v[out.len()],
                    None => l[rng.below(l.len() as u32) as usize],
                };
                g.step(a, &mut keys).unwrap();
                out.push(a);
            }
            (out, keys, g.g.fingerprint(), g.m.results)
        };
        let a = play(None);
        let b = play(Some(&a.0));
        assert_eq!(a, b, "seed {seed}");
    }
}
