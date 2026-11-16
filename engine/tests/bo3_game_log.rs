//! The best-of-three game log (ENGINE.md 168.2, `az/game_log.rs`): every finished game of a live constructed
//! best-of-three match writes one line - both 60s, the starter, the score before it, the dealt flags, how each 60 was
//! made, the result -, worlds and clones never do. zero-core's self-play driver plays the matches (live games from
//! `new_game`, worlds dealt by every search, chained episodes), so the lines are exactly the matches' played games, and a
//! choice table seat's 60 is its context's. Its own binary: the log is a process-wide sink.

use engine::az::constructed::Decklist;
use engine::az::sbtable::context_index;
use engine::az::{game_log, MtgAz};
use serde_json::{json, Value};
use zero_search::driver::{Driver, DriverCfg, KeyKind, NnSeat, SeatCfg};
use zero_search::game::MAX_SEATS;

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

#[test]
fn every_played_game_of_a_live_match_writes_one_line() {
    let d = Decklist::izzet_delver();
    let main = d.main_names();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    // a choice table: every context the main with its first sideboard entry in for its first main entry, but game 3's
    // contexts the main itself (list order: first seen in main then sideboard)
    let (mut names, mut copies, mut base): (Vec<String>, Vec<u64>, Vec<u64>) = (Vec::new(), Vec::new(), Vec::new());
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
    let sb_entry = (0..names.len()).find(|&k| base[k] == 0).unwrap();
    let mut g2 = base.clone();
    g2[0] -= 1;
    g2[sb_entry] += 1;
    let mut choices = Vec::new();
    for game in 2..=3u8 {
        for first in 0..2u8 {
            for lead in -1..=1i8 {
                for od in 0..2u8 {
                    choices.push(json!([game, first, lead, od, if game == 2 { g2.clone() } else { base.clone() }]));
                }
            }
        }
    }
    let table = json!({"version": "vlog", "list": names.iter().zip(&copies).map(|(n, k)| json!([n, k])).collect::<Vec<_>>(), "choices": choices});
    let spec = json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true, "start_game": 2,
        "dealt_decks": {"p": 0.5, "swaps": [0, 15], "per_seat": true}, "sideboard_table": table,
        "decks": {"p0": main, "p1": main, "p0_sideboard": side, "p1_sideboard": side}});
    let dir = std::env::temp_dir().join(format!("mtgz-bo3-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("games-1.jsonl");
    game_log::set(Some(path.to_str().unwrap())).unwrap();
    let mut s = NnSeat::eval(0, 4);
    s.selfplay = true;
    s.temp_moves = 1000;
    let n = 12;
    let cfg = DriverCfg {
        spec: spec.to_string(),
        seats: vec![SeatCfg::Nn(s)],
        games: vec![vec![0, 0]; n],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: n,
        seed: 1682,
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
        max_row_tokens: None,
    };
    let mut drv = Driver::<MtgAz>::new(cfg).unwrap();
    let mut rows = Vec::new();
    while let Some(b) = drv.next_batch(true).unwrap() {
        answer(&mut drv, &b);
        rows.extend(drv.take_results());
        drv.take_samples();
    }
    rows.extend(drv.take_results());
    game_log::set(None).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let spec_lines: Vec<&Value> = lines.iter().filter(|l| l["kind"] == "bo3-spec").collect();
    let games: Vec<&Value> = lines.iter().filter(|l| l["kind"] == "bo3").collect();
    assert_eq!(spec_lines.len(), 1, "one spec line");
    assert_eq!(spec_lines[0]["table"], "vlog");
    // one line per played game: the matches' episodes (chained: one per game played)
    let played: usize = rows.iter().map(|r| r.episodes.len()).sum();
    assert!(rows.iter().all(|r| r.over), "every match finished");
    assert_eq!(games.len(), played, "one line per played game ({} matches)", rows.len());
    let list: Vec<(String, u64)> = spec_lines[0]["lists"][0].as_array().unwrap().iter().map(|e| (e[0].as_str().unwrap().into(), e[1].as_u64().unwrap())).collect();
    let mut table_seats = 0;
    for g in &games {
        let game = g["game"].as_u64().unwrap() as u8;
        assert!((2..=3).contains(&game) && g["post_board"] == true);
        for p in 0..2 {
            let deck: Vec<u64> = g["decks"][p].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
            assert_eq!(deck.iter().sum::<u64>(), 60);
            let dealt = g["dealt"][p].as_bool().unwrap();
            let mode = g["mode"][p].as_str().unwrap();
            assert_eq!(mode, if dealt { "dealt" } else { "table" }, "both seats are the table's 75: a seat not dealt is a table seat");
            if mode == "table" {
                // its context: the next game, sideboarded first = this game's chooser, its lead before the game
                let first = g["chooser"].as_u64().unwrap() as usize == p;
                let w = g["wins"].as_array().unwrap();
                let lead = (w[p].as_i64().unwrap() - w[1 - p].as_i64().unwrap()).signum() as i8;
                let od = g["dealt"][1 - p].as_bool().unwrap();
                let _ = context_index(game, first, lead, od);
                let want: Vec<u64> = list.iter().map(|(name, _)| {
                    let k = names.iter().position(|x| x == name).unwrap();
                    if game == 2 { g2[k] } else { base[k] }
                }).collect();
                assert_eq!(deck, want, "a table seat's 60 is its context's (game {game})");
                table_seats += 1;
            }
        }
        assert!(g["result"].is_array());
    }
    assert!(table_seats > 0, "no table seat in {} games", games.len());
    let _ = std::fs::remove_dir_all(&dir);
}
