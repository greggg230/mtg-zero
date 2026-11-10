//! The EoE sealed game log (ENGINE.md 107.1, `az/game_log.rs`): every finished live `mtg_eoe` game
//! writes one line - its decks, their builder's mode, the result -, worlds never do, and a game left
//! unfinished writes none. One test in its own binary: the log is a process-wide sink.

use engine::az::eoe::MtgEoeAz;
use engine::az::{game_log, sealed, MtgSpec};
use engine::rng::Rng;
use zero_search::game::SearchGame;

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
            g.step(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
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
    let modes: Vec<&str> = games.iter().flat_map(|l| [l["seats"][0]["mode"].as_str().unwrap(), l["seats"][1]["mode"].as_str().unwrap()]).collect();
    println!("{} games logged, modes {modes:?}", games.len());
    let _ = std::fs::remove_dir_all(&dir);
}
