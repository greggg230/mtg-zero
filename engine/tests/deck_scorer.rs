//! The deck choice by an evaluator (ENGINE.md 112): the exported evaluator's scorer against torch's scores (the
//! fixtures `tests/data/scorer_*.json`, made by `python -m mtgz.sealed.scorer fixtures`), its loud refusals, the spec
//! key, the choice (the best candidate of the seat's own pool, no decision, a function of the seed and the file), the
//! prior of a choosing seat (its samples choose as the seat does), and the game log (the evaluator's sha256 and score
//! beside the chosen deck; the game replays from its spec line).

use engine::az::eoe::MtgEoeAz;
use engine::az::hidden_decks::{check_sample, DeckPrior, DeckWant, SealedPrior};
use engine::az::sealed::{self, builder, scorer, Mode, SealedSpec};
use engine::az::{MtgAz, MtgSpec};
use engine::rng::Rng;
use zero_search::game::SearchGame;

fn data(name: &str) -> String {
    format!("{}/tests/data/{name}", env!("CARGO_MANIFEST_DIR")).replace('\\', "/")
}

fn ids(names: &[serde_json::Value]) -> Vec<u16> {
    names.iter().map(|n| sealed::db().expect_id(n.as_str().unwrap()).unwrap()).collect()
}

fn choice_spec(kind: &str, k: u8, explore: f64, extra: &str) -> MtgSpec {
    let path = data(&format!("scorer_{kind}.json"));
    MtgSpec::parse(&format!(
        r#"{{"sealed": {{"set": "eoe", "collation": "paper", "deck_choice": {{"k": {k}, "explore": {explore}, "by": "evaluator", "evaluator": "{path}"{extra}}}}}, "max_turns": 30}}"#
    ))
    .unwrap()
}

/// The fixtures: `python -m mtgz.sealed.scorer fixtures` (random evaluators of every kind, exported by
/// `mtgz.sealed.evaluator`'s `Evaluator.export`, and torch's scores of 48 decks).
const FIXTURES: [&str; 4] = ["linear", "set", "pair", "pair_linear"];

#[test]
fn the_scorer_scores_as_torch_does() {
    let check: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data("scorer_check.json")).unwrap()).unwrap();
    let decks: Vec<Vec<u16>> = check["decks"].as_array().unwrap().iter().map(|d| ids(d.as_array().unwrap())).collect();
    assert!(decks.len() >= 40);
    for name in FIXTURES {
        let path = data(&format!("scorer_{name}.json"));
        let s = scorer::load(&path, sealed::db()).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(s.sha256, scorer::sha256(&bytes), "the sha256 of the bytes");
        match name {
            "pair" => assert_eq!((s.kind, s.field_decks()), (scorer::Kind::Pair { base: scorer::Base::Set { d: 4, k: 3, hidden: 8 }, dim: 3 }, 24)),
            "pair_linear" => assert_eq!((s.kind, s.field_decks()), (scorer::Kind::Pair { base: scorer::Base::Linear, dim: 0 }, 24)),
            _ => assert_eq!(s.field_decks(), 0),
        }
        let want: Vec<f64> = check[name].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
        let mut worst = 0.0f64;
        for (d, w) in decks.iter().zip(&want) {
            let got = s.score(d).unwrap();
            worst = worst.max((got - w).abs());
        }
        println!("{name}: {} decks, the largest |engine - torch| {worst:.2e}", decks.len());
        assert!(worst < 1e-5, "{name}: {worst}");
        // the deck's order does not matter, and the spread of the scores is real (a fixture that tests something)
        let mut rev = decks[0].clone();
        rev.reverse();
        assert_eq!(s.score(&rev).unwrap(), s.score(&decks[0]).unwrap());
        let (lo, hi) = want.iter().fold((1.0f64, 0.0f64), |(a, b), &x| (a.min(x), b.max(x)));
        assert!(hi - lo > 0.05, "{name}: the fixture's scores spread {lo}..{hi}");
        let again = scorer::load(&path, sealed::db()).unwrap();
        assert!(std::sync::Arc::ptr_eq(&s, &again) || again.sha256 == s.sha256, "read once per process (or again, the same)");
    }
}

#[test]
fn the_scorer_refuses_what_it_cannot_score() {
    let db = sealed::db();
    let base: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data("scorer_set.json")).unwrap()).unwrap();
    let pair: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(data("scorer_pair.json")).unwrap()).unwrap();
    let bad = |v0: &serde_json::Value, f: &dyn Fn(&mut serde_json::Value), what: &str| {
        let mut v = v0.clone();
        f(&mut v);
        let e = scorer::Scorer::parse(v.to_string().as_bytes(), "x.json", db).unwrap_err();
        assert!(e.contains(what), "{what}: {e}");
    };
    let pop = |v: &mut serde_json::Value| v.as_array_mut().unwrap().pop().map(|_| ()).unwrap();
    bad(&base, &|v| v["format"] = serde_json::json!("other"), "not an mtgz-deck-evaluator-export");
    bad(&base, &|v| v["version"] = serde_json::json!(2), "version");
    bad(&base, &|v| v["kind"] = serde_json::json!("triple"), "kind \"triple\"");
    bad(&base, &|v| v["features"]["names"][3] = serde_json::json!("x"), "deck features");
    bad(&base, &|v| v["features"]["std"][0] = serde_json::json!(0.0), "std");
    bad(&base, &|v| v["cards"]["names"][0] = serde_json::json!("No Such Card"), "not a card of the EoE table");
    bad(&base, &|v| v["cards"]["names"][1] = v["cards"]["names"][0].clone(), "twice");
    bad(&base, &|v| v["cards"]["land"][0] = serde_json::json!(0.5), "not 0 or 1");
    bad(&base, &|v| pop(&mut v["cards"]["mv"]), "cards.mv");
    bad(&base, &|v| pop(&mut v["members"][0]["wc"]), "wc");
    bad(&base, &|v| pop(&mut v["members"][1]["emb"]), "emb");
    bad(&base, &|v| v["members"][0]["b1"] = serde_json::json!([1.0]), "b1");
    bad(&base, &|v| v["members"] = serde_json::json!([]), "no members");
    bad(&base, &|v| v["hyper"] = serde_json::json!({}), "hyper.d");
    bad(&pair, &|v| v["field"] = serde_json::Value::Null, "without a stored field");
    bad(&pair, &|v| v["field"]["decks"][0][0] = serde_json::json!([9999, 1]), "field deck 0");
    bad(&pair, &|v| v["hyper"]["base"] = serde_json::json!("deep"), "hyper.base");
    bad(&pair, &|v| v["members"][0]["strength"]["bias"] = serde_json::json!(0.5), "no bias");
    bad(&pair, &|v| pop(&mut v["members"][0]["W"]), "W");
    let e = scorer::load(&data("no_such_scorer.json"), db).unwrap_err();
    assert!(e.contains("no_such_scorer"), "{e}");
    // a card out of the table: an error at scoring, never a dropped card; Paradox Haze is in the registry's order but
    // not the table's (`in_table` 0)
    let mut v = base.clone();
    let name = v["cards"]["names"][0].as_str().unwrap().to_string();
    v["cards"]["in_table"][0] = serde_json::json!(0.0);
    let s = scorer::Scorer::parse(v.to_string().as_bytes(), "short.json", db).unwrap();
    let card = db.expect_id(&name).unwrap();
    assert!(!s.has(card) && s.score(&[card; 40]).unwrap_err().contains("not in its table"));
    let names: Vec<&str> = base["cards"]["names"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
    let haze = names.iter().position(|&n| n == "Paradox Haze").expect("Paradox Haze has its line");
    assert_eq!(base["cards"]["in_table"][haze].as_f64(), Some(0.0));
}

#[test]
fn the_evaluator_choice_key_parses_and_refuses() {
    for kind in ["linear", "set", "pair"] {
        let s = choice_spec(kind, 8, 0.25, "");
        let sl = s.sealed.as_deref().unwrap();
        let dc = sl.deck_choice.as_ref().unwrap();
        let e = dc.evaluator().expect("by an evaluator");
        assert_eq!((dc.k, dc.explore.0, e.temperature, dc.seats), (8, 0.25, 0.0, [true, true]));
        assert_eq!(e.scorer.path, data(&format!("scorer_{kind}.json")));
        assert_eq!(SealedSpec::parse(&sl.to_json()).unwrap(), *sl, "the JSON round trip");
        let j = sl.to_json();
        assert_eq!(j["deck_choice"]["evaluator_sha"], serde_json::json!(e.scorer.sha_hex()));
        assert_eq!(j["deck_choice"]["by"], "evaluator");
    }
    // the value choice's JSON as before (no key of the evaluator's), seats only when not both
    let v = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena", "deck_choice": {"k": 4}}}"#).unwrap();
    assert_eq!(v.sealed.as_deref().unwrap().to_json()["deck_choice"], serde_json::json!({"k": 4, "explore": 0.1}));
    let v = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena", "deck_choice": {"k": 4, "by": "value", "seats": [1]}}}"#).unwrap();
    let dc = v.sealed.as_deref().unwrap().deck_choice.clone().unwrap();
    assert_eq!((dc.seats, dc.evaluator().is_none()), ([false, true], true));
    assert_eq!(v.sealed.as_deref().unwrap().to_json()["deck_choice"], serde_json::json!({"k": 4, "explore": 0.1, "seats": [1]}));
    // a pinned sha256, a temperature
    let sha = scorer::sha256(&std::fs::read(data("scorer_set.json")).unwrap());
    let ok = choice_spec("set", 4, 0.0, &format!(r#", "evaluator_sha": "{}", "temperature": 0.02, "seats": [0]"#, scorer::hex(&sha)));
    let dc = ok.sealed.as_deref().unwrap().deck_choice.clone().unwrap();
    assert_eq!((dc.evaluator().unwrap().temperature, dc.seats), (0.02, [true, false]));
    let path = data("scorer_set.json");
    let refused = |dc: String, what: &str| {
        let e = MtgSpec::parse(&format!(r#"{{"sealed": {{"set": "eoe", "collation": "arena", "deck_choice": {dc}}}}}"#)).unwrap_err();
        assert!(e.contains(what), "{dc}: {e}");
    };
    refused(format!(r#"{{"by": "evaluator", "evaluator": "{path}", "evaluator_sha": "{}"}}"#, "0".repeat(64)), "not the spec's");
    refused(r#"{"by": "evaluator"}"#.into(), "needs \"evaluator\"");
    refused(format!(r#"{{"evaluator": "{path}"}}"#), "deck choice by an evaluator's");
    refused(r#"{"by": "network"}"#.into(), "by must be");
    refused(r#"{"by": "evaluator", "evaluator": "C:/no/such/file.json"}"#.into(), "file.json");
    refused(format!(r#"{{"by": "evaluator", "evaluator": "{path}", "temperature": -1}}"#), "temperature");
    refused(format!(r#"{{"by": "evaluator", "evaluator": "{path}", "seats": [0, 0]}}"#), "seats");
    refused(format!(r#"{{"by": "evaluator", "evaluator": "{path}", "seats": [2]}}"#), "seats");
    refused(format!(r#"{{"by": "evaluator", "evaluator": "{path}", "evaluator_sha": "abc"}}"#), "64 hex");
}

#[test]
fn the_evaluator_chooses_the_best_candidate_of_the_seats_own_pool() {
    let db = sealed::db();
    for kind in ["linear", "set", "pair"] {
        let s = choice_spec(kind, 8, 0.25, "");
        let sl = s.sealed.as_deref().unwrap();
        let dc = sl.deck_choice.as_ref().unwrap();
        let sc = &dc.evaluator().unwrap().scorer;
        let (mut chosen, mut explored, mut not_first) = (0, 0, 0);
        for seed in 0..40u64 {
            let a = MtgAz::new(&s, seed).unwrap();
            assert!(a.deck_choice().is_none() && !a.value_choice(), "no decision: the game begins");
            let b = MtgAz::new(&s, seed).unwrap();
            for p in 0..2u8 {
                assert_eq!(a.m.deck(p), b.m.deck(p), "a function of the seed and the file");
                let (cands, ps) = sl.candidates(seed, p).unwrap().unwrap();
                let how = a.m.how[p as usize].unwrap();
                assert_eq!(how.pool_seed, ps);
                match sealed::explore_pick(seed, p, dc.explore.0, dc.k) {
                    Some(j) => {
                        explored += 1;
                        assert_eq!((how.mode, how.candidate, how.evaluator, how.score), (Mode::Explore, Some(j), None, None));
                        assert_eq!(a.m.deck(p), cands[j as usize].deck);
                    }
                    None => {
                        chosen += 1;
                        let scores: Vec<f64> = cands.iter().map(|c| sc.score(&c.deck).unwrap()).collect();
                        let j = (0..scores.len()).fold(0, |b, i| if scores[i] > scores[b] { i } else { b });
                        not_first += (j != 0) as u32;
                        assert_eq!((how.mode, how.candidate, how.evaluator), (Mode::Chosen, Some(j as u8), Some(sc.sha256)));
                        assert_eq!(how.score.map(f64::from_bits), Some(scores[j]));
                        assert_eq!(a.m.deck(p), cands[j].deck);
                        // the game is played with it: the starting library and hand are the chosen deck
                        let g = &a.g;
                        let mut d: Vec<u16> = g.player(p).library.iter().chain(g.player(p).hand.iter()).map(|&c| g.obj(c).def).collect();
                        d.sort_unstable();
                        assert_eq!(d, cands[j].deck);
                    }
                }
            }
            assert_eq!(a.m.deck_priors[0].as_ref().unwrap().name(), "sealed+evaluator");
        }
        println!("{kind}: {chosen} seats chose ({not_first} not the best guess), {explored} explored");
        assert!(chosen > 40 && explored > 5 && not_first > 5, "{chosen} {explored} {not_first}");
        // no hidden information: a seat's choice reads its own pool only (the other seat given two different pools)
        for seed in 0..12u64 {
            let other: Vec<serde_json::Value> = SealedSpec::parse(&serde_json::json!({"set": "eoe", "collation": "paper"}))
                .unwrap()
                .pool_of(seed + 100, 1)
                .unwrap()
                .unwrap()
                .0
                .iter()
                .map(|&d| serde_json::json!(db.def(d).name))
                .collect();
            let mut v: serde_json::Value = serde_json::from_str(&format!(r#"{{"sealed": {}, "max_turns": 30}}"#, sl.to_json())).unwrap();
            let x = MtgAz::new(&MtgSpec::parse(&v.to_string()).unwrap(), seed).unwrap();
            v["sealed"]["p1"] = serde_json::json!({"pool": other});
            let y = MtgAz::new(&MtgSpec::parse(&v.to_string()).unwrap(), seed).unwrap();
            assert_eq!(x.m.deck(0), y.m.deck(0), "seed {seed}: seat 0's choice does not see seat 1's pool");
            assert_eq!(x.m.how[0], y.m.how[0]);
        }
    }
    // `seats`: only seat 0 chooses; seat 1 plays the spec's seat (here the best guess: best_share 1)
    let s = MtgSpec::parse(&format!(
        r#"{{"sealed": {{"set": "eoe", "collation": "paper", "best_share": 1.0, "deck_choice": {{"k": 8, "explore": 0.0, "by": "evaluator", "evaluator": "{}", "seats": [0]}}}}}}"#,
        data("scorer_set.json")
    ))
    .unwrap();
    let sl = s.sealed.as_deref().unwrap();
    for seed in 0..10u64 {
        let a = MtgAz::new(&s, seed).unwrap();
        assert_eq!(a.m.how[0].unwrap().mode, Mode::Chosen);
        assert_eq!(a.m.how[1].unwrap().mode, Mode::Best);
        assert_eq!(a.m.deck(1), sl.seat(seed, 1).unwrap().0.deck);
        assert!(sl.candidates(seed, 1).unwrap().is_none());
        assert_eq!(a.m.deck_priors[0].as_ref().unwrap().name(), "sealed+evaluator");
        assert_eq!(a.m.deck_priors[1].as_ref().unwrap().name(), "sealed", "seat 1's prior: the mixed field's");
    }
}

#[test]
fn the_temperature_draws_by_the_softmax_of_the_scores() {
    let scores = [0.50, 0.52, 0.47, 0.52];
    assert_eq!(sealed::pick_by_score(&scores, 0.0, &mut Rng::new(1)), 1, "argmax: the first of the best");
    let t = 0.02;
    let w: Vec<f64> = scores.iter().map(|s| ((s - 0.52) / t).exp()).collect();
    let z: f64 = w.iter().sum();
    let mut n = [0u32; 4];
    let mut rng = Rng::new(7);
    let draws = 40_000;
    for _ in 0..draws {
        n[sealed::pick_by_score(&scores, t, &mut rng)] += 1;
    }
    for i in 0..4 {
        let p = w[i] / z;
        let se = (p * (1.0 - p) / draws as f64).sqrt();
        assert!((n[i] as f64 / draws as f64 - p).abs() < 4.0 * se + 1e-9, "{i}: {} vs {p}", n[i]);
    }
    // a temperature in a game: the seat's own generator, the same pick from the same seed
    let s = choice_spec("set", 8, 0.0, r#", "temperature": 0.05"#);
    let picks: Vec<Option<u8>> = (0..20u64).map(|seed| MtgAz::new(&s, seed).unwrap().m.how[0].unwrap().candidate).collect();
    let again: Vec<Option<u8>> = (0..20u64).map(|seed| MtgAz::new(&s, seed).unwrap().m.how[0].unwrap().candidate).collect();
    assert_eq!(picks, again);
}

#[test]
fn the_prior_of_a_choosing_seat_chooses_as_the_seat_does() {
    let db = sealed::db();
    let s = choice_spec("set", 8, 0.0, "");
    let sl = s.sealed.as_deref().unwrap();
    let sc = sl.deck_choice.as_ref().unwrap().evaluator().unwrap().scorer.clone();
    let prior = SealedPrior::of_seat(sl, 0);
    assert_eq!(prior.name(), "sealed+evaluator");
    // nothing shown: the sample is the best of K candidates of its pool - the best guess and K - 1 randomised builds
    // drawn from the generator - exactly
    let empty = DeckWant { db, n: 40, shown: &[], companion: None, tilt: None };
    for i in 0..24u64 {
        let pool = sl.pool_of(1000 + i, 0).unwrap().unwrap().0;
        let got = prior.sample_from(&empty, Some(&pool), &mut Rng::new(i));
        check_sample(&prior, &empty, &got).unwrap();
        let mut rng = Rng::new(i);
        let mut cands = vec![builder::best_guess(db, &pool, &sl.builder)];
        for _ in 1..8 {
            cands.push(builder::build(db, &pool, &sl.builder, &mut rng));
        }
        let scores: Vec<f64> = cands.iter().map(|c| sc.score(&c.deck).unwrap()).collect();
        let j = (0..8).fold(0, |b, x| if scores[x] > scores[b] { x } else { b });
        assert_eq!(got.deck, cands[j].deck, "sample {i}: the evaluator's best candidate");
    }
    // the field's choice is the seat's: a chosen deck's score is the best of its pool's candidates, and the prior's
    // samples (nothing shown) score as high on average as the real chosen decks (both the evaluator's best of K)
    let mut rng = Rng::new(99);
    let (mut prior_mean, mut plain_mean) = (0.0, 0.0);
    let plain = SealedPrior::of_spec(sl);
    for _ in 0..60 {
        prior_mean += sc.score(&prior.sample(&empty, &mut rng).deck).unwrap() / 60.0;
        plain_mean += sc.score(&plain.sample(&empty, &mut rng).deck).unwrap() / 60.0;
    }
    let real_mean = (0..60u64).map(|seed| f64::from_bits(MtgAz::new(&s, seed).unwrap().m.how[0].unwrap().score.unwrap())).sum::<f64>() / 60.0;
    println!("mean score: the choosing prior {prior_mean:.4}, the real chosen decks {real_mean:.4}, the mixed field's prior {plain_mean:.4}");
    assert!(prior_mean > plain_mean, "the choosing prior samples better-scored decks than the plain one");
    assert!((prior_mean - real_mean).abs() < 0.03, "the prior's chosen decks score as the field's: {prior_mean} vs {real_mean}");
    // shown cards of real chosen decks: the contract, and the deck in the shown cards' colours (a pair and a splash)
    let cols = |v: &[u16]| v.iter().filter(|&&d| !db.def(d).is(engine::db::types::LAND)).fold(0u8, |m, &d| m | db.def(d).colors) & 31;
    let (mut inside, mut many, mut exact) = (0, 0, 0);
    for seed in 0..60u64 {
        let real = MtgAz::new(&s, seed).unwrap().m.deck(0);
        let k = (seed % 21) as usize;
        let mut shown: Vec<u16> = real.iter().copied().step_by(2).take(k).collect();
        shown.sort_unstable();
        let want = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None };
        let got = prior.sample(&want, &mut Rng::new(seed));
        check_sample(&prior, &want, &got).unwrap();
        // (with 8 or more cards shown, the real deck's colours are known: the sample is a deck of them and a splash at
        // most - a covering candidate may splash, as 102.6's samples may)
        if k >= 8 {
            many += 1;
            inside += ((cols(&got.deck) & !cols(&real)).count_ones() <= 1) as u32;
            exact += (cols(&got.deck) & !cols(&real) == 0) as u32;
        }
    }
    println!("shown cards of real chosen decks, 8+ shown: {inside} of {many} samples in the real deck's colours and a splash at most, {exact} without");
    assert!(inside * 10 >= many * 9, "{inside} of {many}");
    // the explore share: at 1 the samples are the randomised builds, the choice never made
    let s1 = choice_spec("set", 8, 1.0, "");
    let all_explore = SealedPrior::of_seat(s1.sealed.as_deref().unwrap(), 1);
    for i in 0..10u64 {
        assert_eq!(all_explore.sample(&empty, &mut Rng::new(i)), SealedPrior::of_spec(s1.sealed.as_deref().unwrap()).sample(&empty, &mut Rng::new(i)));
    }
}

#[test]
fn the_game_log_records_the_evaluator_and_replays() {
    let dir = std::env::temp_dir().join(format!("mtgz-deck-scorer-log-{}", std::process::id()));
    let path = dir.join("games.jsonl");
    let _ = std::fs::remove_dir_all(&dir);
    let s = choice_spec("set", 6, 0.25, "");
    let sl = s.sealed.as_deref().unwrap();
    let sha = sl.deck_choice.as_ref().unwrap().evaluator().unwrap().scorer.sha_hex();
    engine::az::game_log::set(Some(path.to_str().unwrap())).unwrap();
    let mut rng = Rng::new(5);
    let mut hows = Vec::new();
    for seed in 0..4u64 {
        let mut g = MtgEoeAz::new_game(&s, 2, seed).unwrap();
        hows.push(g.0.m.how);
        let mut keys = Vec::new();
        while g.to_move().is_some() {
            let mut l = Vec::new();
            g.legal_actions(&mut l);
            g.step(l[rng.below(l.len() as u32) as usize], &mut keys).unwrap();
        }
    }
    engine::az::game_log::set(None).unwrap();
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let spec_line = lines.iter().find(|x| x.get("sealed").is_some()).unwrap();
    assert_eq!(spec_line["sealed"]["deck_choice"]["evaluator_sha"], serde_json::json!(sha), "the spec line names the file and its sha256");
    let games: Vec<&serde_json::Value> = lines.iter().filter(|l| l.get("seats").is_some()).collect();
    assert_eq!(games.len(), 4);
    let mut chosen = 0;
    for (seed, (l, how)) in games.iter().zip(&hows).enumerate() {
        for p in 0..2 {
            let h = how[p].unwrap();
            let seat = &l["seats"][p];
            assert_eq!(seat["mode"], h.mode.name());
            assert_eq!(seat["candidate"], serde_json::json!(h.candidate));
            if h.mode == Mode::Chosen {
                chosen += 1;
                assert_eq!(seat["evaluator"], serde_json::json!(sha), "the evaluator's sha256 beside the chosen deck");
                assert!((seat["score"].as_f64().unwrap() - f64::from_bits(h.score.unwrap())).abs() < 1e-12);
            } else {
                assert!(seat.get("evaluator").is_none());
            }
        }
        // the exact replay from the spec line (which names the evaluator file and pins its sha256), the seed and the actions
        let mut sp = lines.iter().find(|x| x.get("sealed").is_some() && x["spec"] == l["spec"]).unwrap().as_object().unwrap().clone();
        sp.remove("v");
        sp.remove("spec");
        let rs = MtgSpec::parse(&serde_json::Value::Object(sp).to_string()).unwrap();
        let mut r = MtgEoeAz::new_game(&rs, 2, seed as u64).unwrap();
        assert_eq!(r.0.m.how, *how, "the same choice from the spec line");
        let n = l["n_actions"].as_u64().unwrap() as usize;
        let raw = l["actions"].as_str().unwrap();
        let acts = decode_actions(raw);
        assert_eq!(acts.len(), n);
        for &a in &acts {
            r.step(a, &mut Vec::new()).unwrap();
        }
        assert!(r.to_move().is_none());
        assert_eq!(serde_json::json!([r.result()[0], r.result()[1]]), l["result"]);
    }
    assert!(chosen >= 4, "{chosen}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The actions of a game line (little-endian u16, standard base64).
fn decode_actions(s: &str) -> Vec<u16> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {c}"),
        }
    };
    let mut out = Vec::new();
    for ch in s.as_bytes().chunks(4) {
        let pad = ch.iter().filter(|&&c| c == b'=').count();
        let n = ch.iter().take(4 - pad).enumerate().fold(0u32, |n, (i, &c)| n | val(c) << (18 - 6 * i));
        for k in 0..3 - pad {
            out.push((n >> (16 - 8 * k)) as u8);
        }
    }
    out.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

/// What the deck choice by an evaluator costs (ENGINE.md 112; `cargo test --release -p engine --test deck_scorer
/// choice_cost -- --ignored --nocapture`): a game's creation with K = 32 candidates chosen by each fixture evaluator
/// against the value choice's (the candidates, no scoring), and a new request's K-sample deal of the choosing prior
/// against the mixed field's.
#[test]
#[ignore]
fn choice_cost() {
    let db = sealed::db();
    let time = |f: &mut dyn FnMut()| {
        let t = std::time::Instant::now();
        f();
        t.elapsed().as_secs_f64()
    };
    let n = 40u64;
    let value = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "arena", "best_share": 0.5, "deck_choice": {"k": 32, "explore": 0.1}}, "max_turns": 60}"#).unwrap();
    let mut out = vec![("value choice (candidates only)".to_string(), time(&mut || (0..n).for_each(|s| drop(MtgAz::new(&value, s).unwrap()))) / n as f64)];
    // (the fixtures, and `DECK_SCORER=<file>`: a real evaluator's, e.g. run 1's v1 set)
    let mut files: Vec<(String, String)> = ["linear", "set"].iter().map(|k| (k.to_string(), data(&format!("scorer_{k}.json")))).collect();
    if let Ok(p) = std::env::var("DECK_SCORER") {
        files.push(("DECK_SCORER".into(), p));
    }
    for (kind, file) in files {
        let s = MtgSpec::parse(&format!(
            r#"{{"sealed": {{"set": "eoe", "collation": "arena", "best_share": 0.5, "deck_choice": {{"k": 32, "explore": 0.1, "by": "evaluator", "evaluator": "{file}"}}}}, "max_turns": 60}}"#
        ))
        .unwrap();
        out.push((format!("evaluator choice ({kind})"), time(&mut || (0..n).for_each(|x| drop(MtgAz::new(&s, x).unwrap()))) / n as f64));
        let sl = s.sealed.as_deref().unwrap();
        let sc = sl.deck_choice.as_ref().unwrap().evaluator().unwrap().scorer.clone();
        let deck = MtgAz::new(&s, 0).unwrap().m.deck(0);
        out.push((format!("one score ({kind})"), time(&mut || (0..1000).for_each(|_| { std::hint::black_box(sc.score(&deck).unwrap()); })) / 1000.0));
        let prior = SealedPrior::of_seat(sl, 0);
        let plain = SealedPrior::of_spec(sl);
        let pools: Vec<Vec<u16>> = (0..32u64).map(|i| sl.pool_of(5000 + i, 0).unwrap().unwrap().0).collect();
        for (name, pr) in [(format!("a request's 32 deals, choosing prior ({kind})"), &prior as &dyn DeckPrior), ("a request's 32 deals, mixed field's prior".to_string(), &plain as &dyn DeckPrior)] {
            let real = MtgAz::new(&s, 1).unwrap().m.deck(0);
            let shown: Vec<u16> = real.iter().copied().step_by(3).take(6).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            let want = DeckWant { db, n: 40, shown: &shown, companion: None, tilt: None };
            let mut rng = Rng::new(3);
            out.push((name, time(&mut || pools.iter().for_each(|p| drop(pr.sample_from(&want, Some(p), &mut rng)))) / 32.0 * 32.0));
        }
    }
    for (what, secs) in out {
        println!("{what}: {:.3} ms", secs * 1e3);
    }
}

/// The hidden decklists' prior per game (ENGINE.md 112; `cargo test --release -p engine --test deck_scorer
/// prior_cost_per_game -- --ignored --nocapture`, `DECK_SCORER=<an exported evaluator>` for a real one, else the
/// set fixture): heuristic games of eoe-2's spec (the value choice: the mixed field's prior) and of eoe-2e's (the
/// evaluator's choice: the choosing prior), the decider dealing one world at every decision as its search's first
/// deal does - the time of the deals (each new request's K decks drawn, then the memo's) and the requests per game.
#[test]
#[ignore]
fn prior_cost_per_game() {
    use zero_search::game::make_bot;
    let path = std::env::var("DECK_SCORER").unwrap_or_else(|_| data("scorer_set.json"));
    let n = std::env::var("PRIOR_COST_GAMES").ok().map_or(16u64, |v| v.parse().unwrap());
    let specs = [
        ("value choice: the mixed field's prior", r#"{"sealed": {"set": "eoe", "collation": "arena", "best_share": 0.5, "deck_choice": {"k": 32, "explore": 1.0}}, "max_turns": 60}"#.to_string()),
        ("evaluator choice: the choosing prior", format!(r#"{{"sealed": {{"set": "eoe", "collation": "arena", "best_share": 0.5, "deck_choice": {{"k": 32, "explore": 0.1, "by": "evaluator", "evaluator": "{path}"}}}}, "max_turns": 60}}"#)),
    ];
    for (name, js) in &specs {
        let sp = MtgSpec::parse(js).unwrap();
        let (mut secs, mut decisions, mut turns) = (0.0f64, 0u64, 0u64);
        for s in 0..n {
            let mut g = MtgEoeAz::new_game(&sp, 2, 900 + s).unwrap();
            let mut bots = [make_bot::<MtgEoeAz>(&sp, "heuristic", s).unwrap(), make_bot::<MtgEoeAz>(&sp, "heuristic", s + 1).unwrap()];
            let (mut keys, mut legal) = (Vec::new(), Vec::new());
            let mut zr = zero_search::rng::Rng::new(s);
            while let Some(p) = g.to_move() {
                g.legal_actions(&mut legal);
                let t = std::time::Instant::now();
                let mut w = g.clone();
                w.determinize(p, &mut zr);
                secs += t.elapsed().as_secs_f64();
                decisions += 1;
                let a = bots[p].choose(&g, &legal);
                g.step(a, &mut keys).unwrap();
            }
            turns += g.0.rounds() as u64;
        }
        println!("{name}: {:.1} ms of deals a game ({} decisions, {:.1} rounds a game; {n} games)", secs * 1e3 / n as f64, decisions / n, turns as f64 / n as f64);
    }
}

/// The randomised builder's and the best guess's time per build on arena pools (ENGINE.md 112: the spells' weights
/// computed once per build; `cargo test --release -p engine --test deck_scorer builder_cost -- --ignored --nocapture`).
#[test]
#[ignore]
fn builder_cost() {
    let db = sealed::db();
    let sl = SealedSpec::parse(&serde_json::json!({"set": "eoe", "collation": "arena"})).unwrap();
    let pools: Vec<Vec<u16>> = (0..50u64).map(|i| sl.pool_of(7000 + i, 0).unwrap().unwrap().0).collect();
    let mut rng = Rng::new(1);
    let t = std::time::Instant::now();
    for p in &pools {
        for _ in 0..40 {
            std::hint::black_box(builder::build(db, p, &sl.builder, &mut rng));
        }
    }
    println!("build: {:.1} us", t.elapsed().as_secs_f64() * 1e6 / 2000.0);
    let t = std::time::Instant::now();
    for p in &pools {
        for _ in 0..40 {
            std::hint::black_box(builder::best_guess(db, p, &sl.builder));
        }
    }
    println!("best guess: {:.1} us", t.elapsed().as_secs_f64() * 1e6 / 2000.0);
}

/// `"seats"` with the value choice (107.2's decision, ENGINE.md 112.2): only seat 1 chooses - the decision is seat 1's,
/// seat 0 plays the spec's seat - and its choice starts the game.
#[test]
fn the_seats_key_with_the_value_choice() {
    let s = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "paper", "deck_choice": {"k": 4, "explore": 0.0, "seats": [1]}}, "max_turns": 30}"#).unwrap();
    let sl = s.sealed.as_deref().unwrap();
    for seed in 0..6u64 {
        let mut a = MtgAz::new(&s, seed).unwrap();
        let c = a.deck_choice().expect("seat 1 chooses");
        assert_eq!((c.decider(), c.n), (1, 1));
        assert_eq!(a.m.how[0].unwrap().mode, Mode::Explore, "seat 0: the spec's seat (no best_share: the randomised build)");
        assert_eq!(a.m.deck(0), sl.seat(seed, 0).unwrap().0.deck);
        let mut l = Vec::new();
        a.legal_actions(&mut l);
        assert_eq!(l.len(), 4);
        a.step(l[2], &mut Vec::new()).unwrap();
        assert!(a.deck_choice().is_none());
        assert_eq!(a.m.deck(1), sl.candidates(seed, 1).unwrap().unwrap().0[2].deck);
        assert_eq!(a.m.how[1].unwrap().mode, Mode::Chosen);
    }
}
