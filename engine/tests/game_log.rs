//! The EoE sealed game log (ENGINE.md 107.1, 107.3, `az/game_log.rs`): every finished live `mtg_eoe` game
//! writes one line - its decks, their builder's mode, the result, its draws and plays, the actions that
//! replay it exactly -, worlds and clones never do, and a game left unfinished writes none. One test in its
//! own binary: the log is a process-wide sink.

use engine::az::eoe::MtgEoeAz;
use engine::az::{game_log, sealed, MtgSpec};
use engine::rng::Rng;
use zero_search::game::SearchGame;

/// Standard base64 back to bytes (the line's `actions`).
fn unbase64(s: &str) -> Vec<u8> {
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
    out
}

/// The line's card (an index among the seat's sorted deck names, or a name) as a name.
fn card_name(seat: &serde_json::Value, c: &serde_json::Value) -> String {
    match c.as_u64() {
        Some(i) => seat["deck"].as_object().unwrap().keys().nth(i as usize).unwrap().clone(),
        None => c.as_str().unwrap().to_string(),
    }
}

/// A deck as {name: copies}.
fn counts(deck: &[u16]) -> serde_json::Value {
    let db = sealed::db();
    let mut m = serde_json::Map::new();
    for &d in deck {
        let n = &db.def(d).name;
        let c = m.get(n).and_then(|v| v.as_u64()).unwrap_or(0);
        m.insert(n.clone(), serde_json::json!(c + 1));
    }
    serde_json::Value::Object(m)
}

#[test]
fn finished_live_games_write_one_line_each_with_their_decks_modes_and_result() {
    let dir = std::env::temp_dir().join(format!("mtgz-game-log-{}", std::process::id()));
    let path = dir.join("games").join("games-test.jsonl");
    let _ = std::fs::remove_dir_all(&dir);
    let selfplay = MtgSpec::parse(r#"{"sealed": {"set": "eoe", "collation": "paper", "best_share": 0.5}, "max_turns": 24}"#).unwrap();
    let eval = MtgSpec::parse(&MtgSpec::eval_json(r#"{"sealed": {"set": "eoe", "collation": "paper", "best_share": 0.5}, "max_turns": 24, "eval_decks": 4}"#).unwrap()).unwrap();
    // off: no record
    assert!(!MtgEoeAz::new_game(&selfplay, 2, 1).unwrap().logging());
    game_log::set(Some(path.to_str().unwrap())).unwrap();
    assert_eq!(game_log::path().as_deref(), path.to_str());
    assert!(!path.exists(), "the file is made at the first line");
    // a cube game is not sealed: no record
    assert!(<engine::az::MtgAz as SearchGame>::new_game(&MtgSpec::parse("{}").unwrap(), 2, 1).is_ok());
    let mut rng = Rng::new(1070);
    let mut zr = zero_search::rng::Rng::new(1070);
    let mut want: Vec<(u64, bool, [bool; 2], Option<[f32; 2]>, u32)> = Vec::new();
    // each finished game's actions, draws (seat, card) from its own keys, plays, final fingerprint
    let mut traces: Vec<(Vec<u16>, Vec<(u8, u16)>, u32, u64)> = Vec::new();
    for (i, spec) in [&selfplay, &selfplay, &selfplay, &eval, &selfplay, &eval].into_iter().enumerate() {
        let seed = 500 + i as u64;
        let mut g = MtgEoeAz::new_game(spec, 2, seed).unwrap();
        assert!(g.logging());
        let unfinished = i == 4;
        // seat 0 "searches" (deals worlds) in games 0, 3; both in game 2; nobody in 1 and 5
        let searchers: &[usize] = match i {
            0 | 3 => &[0],
            2 => &[0, 1],
            _ => &[],
        };
        let (mut keys, mut legal) = (Vec::new(), Vec::new());
        let mut steps = 0;
        let (mut acts, mut drew, mut plays) = (Vec::new(), Vec::new(), 0u32);
        while g.to_move().is_some() {
            let p = g.to_move().unwrap();
            if searchers.contains(&p) && steps % 7 == 0 {
                let mut w = g.clone();
                assert!(w.logging(), "a clone carries the record until it is dealt");
                w.determinize(p, &mut zr);
                assert!(!w.logging(), "a world drops the record");
                // a world played to its end writes nothing
                let mut wl = Vec::new();
                for _ in 0..40 {
                    if w.to_move().is_none() {
                        break;
                    }
                    w.legal_actions(&mut wl);
                    w.step(wl[rng.below(wl.len() as u32) as usize], &mut keys).unwrap();
                }
            }
            g.legal_actions(&mut legal);
            let act = legal[rng.below(legal.len() as u32) as usize];
            if let Some(d) = g.0.pending_decision() {
                if matches!(d.kind, engine::decision::DecisionKind::Priority | engine::decision::DecisionKind::ChooseGraveyardCard)
                    && matches!(d.choice_of(act), Some(engine::decision::Choice::Cast(_) | engine::decision::Choice::Play(_)))
                {
                    plays += 1;
                }
            }
            let k0 = keys.len();
            g.step(act, &mut keys).unwrap();
            for &k in &keys[k0..] {
                if engine::hidden::key_kind(k) == engine::hidden::KEY_DRAW {
                    drew.push((engine::hidden::key_player(k), engine::hidden::key_payload(k) as u16));
                }
            }
            acts.push(act);
            steps += 1;
            if unfinished && steps == 60 {
                break;
            }
        }
        if unfinished {
            assert!(g.logging(), "an unfinished game keeps its record and writes nothing");
            continue;
        }
        assert!(!g.logging(), "a finished game wrote its line");
        let r = g.result();
        let s = [searchers.contains(&0), searchers.contains(&1)];
        want.push((seed, std::ptr::eq(spec, &eval), s, (!g.aborted()).then_some([r[0], r[1]]), g.rounds()));
        traces.push((acts, drew, plays, g.0.g.fingerprint()));
    }
    game_log::set(None).unwrap();
    assert!(!MtgEoeAz::new_game(&selfplay, 2, 1).unwrap().logging());
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let specs: Vec<&serde_json::Value> = lines.iter().filter(|l| l.get("sealed").is_some()).collect();
    let games: Vec<&serde_json::Value> = lines.iter().filter(|l| l.get("seats").is_some()).collect();
    assert_eq!(specs.len(), 2, "one spec line per spec");
    assert_eq!(games.len(), want.len(), "one line per finished game");
    assert_eq!(specs[0]["decks"], "random");
    assert_eq!(specs[1]["decks"], "eval");
    assert_eq!(specs[0]["sealed"]["best_share"], 0.5);
    for (l, (seed, is_eval, searched, result, turns)) in games.iter().zip(&want) {
        let spec = if *is_eval { &eval } else { &selfplay };
        assert_eq!(l["seed"], *seed);
        assert_eq!(l["spec"], specs[*is_eval as usize]["spec"]);
        let deck_seed = spec.sealed_seed(*seed);
        assert_eq!(l["deck_seed"], deck_seed);
        assert_eq!(l["eval"].is_null(), !is_eval);
        assert_eq!(l["searched"], serde_json::json!(searched));
        assert_eq!(l["result"], serde_json::json!(result));
        assert_eq!(l["turns"], *turns);
        let sl = spec.sealed.as_deref().unwrap();
        for p in 0..2u8 {
            let (seat, info) = sl.seat(deck_seed, p).unwrap();
            assert_eq!(l["seats"][p as usize]["deck"], counts(&seat.deck), "seed {seed} seat {p}: the deck");
            assert_eq!(l["seats"][p as usize]["mode"], info.mode.name());
            assert_eq!(l["seats"][p as usize]["pool_seed"], serde_json::json!(info.pool_seed));
        }
        // the game's own decks are those
        let seats = spec.try_seats(*seed).unwrap();
        for p in 0..2u8 {
            assert_eq!(seats[p as usize].deck, sl.seat(deck_seed, p).unwrap().0.deck);
        }
    }
    // version 2 (ENGINE.md 107.3): the actions, draws and plays; the spec line, the seed and the actions replay the game
    let db = sealed::db();
    let mut bytes = 0usize;
    for (l, (acts, drew, plays, fp)) in games.iter().zip(&traces) {
        assert_eq!(l["v"], 2);
        let raw = unbase64(l["actions"].as_str().unwrap());
        let got: Vec<u16> = raw.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(&got, acts, "the logged actions");
        assert_eq!(l["n_actions"], acts.len());
        let mut sp = specs.iter().find(|s| s["spec"] == l["spec"]).unwrap().as_object().unwrap().clone();
        sp.remove("v");
        sp.remove("spec");
        let spec = MtgSpec::parse(&serde_json::Value::Object(sp).to_string()).unwrap();
        let mut r = MtgEoeAz::new_game(&spec, 2, l["seed"].as_u64().unwrap()).unwrap();
        assert!(!r.logging(), "the log is off");
        let mut keys = Vec::new();
        for &a in &got {
            r.step(a, &mut keys).unwrap();
        }
        assert!(r.to_move().is_none(), "the replay ends where the game ended");
        assert_eq!(r.0.g.fingerprint(), *fp, "the replay is the game, bit for bit");
        assert_eq!(serde_json::json!([r.result()[0], r.result()[1]]), l["result"]);
        // the draws: the game's own, in order, by seat; turn 0 the opening hands' kind 0, later ones 1 or 2
        for p in 0..2usize {
            let seat = &l["seats"][p];
            let logged: Vec<String> = seat["draws"].as_array().unwrap().iter().map(|d| card_name(seat, &d[2])).collect();
            let real: Vec<String> = drew.iter().filter(|x| x.0 as usize == p).map(|x| db.def(x.1).name.clone()).collect();
            assert_eq!(logged, real, "seat {p}'s draws");
            for d in seat["draws"].as_array().unwrap() {
                let (t, k) = (d[0].as_u64().unwrap(), d[1].as_u64().unwrap());
                assert!(if t == 0 { k == 0 } else { k == 1 || k == 2 }, "draw kind {k} on turn {t}");
            }
            assert!(seat["draws"].as_array().unwrap().iter().filter(|d| d[0] == 0).count() >= 7, "an opening hand");
            for b in seat["bottom"].as_array().unwrap() {
                assert!(seat["deck"].get(card_name(seat, b)).is_some());
            }
            assert_eq!(seat["bottom"].as_array().unwrap().len() as u64 > 0, seat["mulligans"].as_u64().unwrap() > 0, "bottomed cards after a mulligan");
        }
        let n_plays: usize = (0..2).map(|p| l["seats"][p]["plays"].as_array().unwrap().len()).sum();
        assert_eq!(n_plays as u32, *plays, "the plays");
        bytes += serde_json::to_string(l).unwrap().len();
    }
    println!("version 2: {} bytes a game line on average ({} actions on average)", bytes / games.len(), traces.iter().map(|t| t.0.len()).sum::<usize>() / traces.len());
    let modes: Vec<&str> = games.iter().flat_map(|l| [l["seats"][0]["mode"].as_str().unwrap(), l["seats"][1]["mode"].as_str().unwrap()]).collect();
    println!("{} games logged, modes {modes:?}", games.len());
    let _ = std::fs::remove_dir_all(&dir);
}
