//! The metagame's game log (ENGINE.md 176, `az/game_log.rs`): every finished best-of-one game of a live metagame
//! game writes one `"kind": "bo1"` line naming its pairing (the index of each seat's 75, the spec line their names and
//! lists), and a metagame best-of-three's `"kind": "bo3"` lines carry the pairing too - the pairing of the game's seed,
//! never a world's or a clone's line. zero-core's self-play driver plays the games. Its own binary: the log is a
//! process-wide sink.

use engine::az::{game_log, MtgAz, MtgSpec};
use serde_json::{json, Value};
use zero_search::driver::{Driver, DriverCfg, KeyKind, NnSeat, SeatCfg};
use zero_search::game::MAX_SEATS;

fn answer(d: &mut Driver<MtgAz>, b: &zero_search::driver::Batch) {
    let mut pri = Vec::new();
    for r in 0..b.b {
        let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
        pri.extend(std::iter::repeat_n(1.0 / k as f32, k));
    }
    let v = vec![0.5; b.b * MAX_SEATS];
    d.submit(b.id, &pri, &v, &vec![0.0; b.b * MAX_SEATS]).unwrap();
}

fn names(file: &str) -> Value {
    let d = engine::az::constructed::Decklist::parse(&std::fs::read_to_string(format!("{}/../data/decks/{file}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
    let side: Vec<String> = d.sideboard.iter().flat_map(|(k, n)| std::iter::repeat_n(n.clone(), *k as usize)).collect();
    json!({"name": file, "main": d.main_names(), "sideboard": side})
}

/// Play `n` games (matches) of `spec` through the driver with the log on; the log's lines.
fn played(spec: &Value, n: usize, tag: &str) -> (Vec<Value>, usize) {
    let dir = std::env::temp_dir().join(format!("mtgz-meta-log-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("games-1.jsonl");
    game_log::set(Some(path.to_str().unwrap())).unwrap();
    let mut s = NnSeat::eval(0, 4);
    s.selfplay = true;
    s.temp_moves = 1000;
    let cfg = DriverCfg {
        spec: spec.to_string(),
        seats: vec![SeatCfg::Nn(s)],
        games: vec![vec![0, 0]; n],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: n,
        seed: 1760,
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
    assert!(rows.iter().all(|r| r.over), "every game finished");
    let played: usize = rows.iter().map(|r| r.episodes.len().max(1)).sum();
    // (no line written: no file)
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    (text.lines().map(|l| serde_json::from_str(l).unwrap()).collect(), played)
}

/// Every game line's pairing is its seed's, and its spec line names that pairing (names, lists).
fn check(spec: &Value, lines: &[Value], kind: &str, played: usize) {
    let sp = MtgSpec::parse(&spec.to_string()).unwrap();
    let games: Vec<&Value> = lines.iter().filter(|l| l["kind"] == kind).collect();
    assert_eq!(games.len(), played, "one {kind} line per played game");
    let spec_lines: Vec<&Value> = lines.iter().filter(|l| l["kind"] == format!("{kind}-spec")).collect();
    let mut cells = std::collections::BTreeSet::new();
    for g in &games {
        let seed = g["seed"].as_u64().unwrap();
        let p = sp.pairing(seed).unwrap();
        assert_eq!(g["pairing"], json!(p.decks), "seed {seed}: the line's pairing is the seed's");
        cells.insert(p.decks);
        let s = spec_lines.iter().find(|s| s["spec"] == g["spec"]).expect("the game's spec line");
        assert_eq!(s["metagame"], p.json(), "seed {seed}: its spec line names the pairing");
        for q in 0..2 {
            let main = spec["metagame"]["decks"][p.decks[q] as usize]["main"].as_array().unwrap();
            let n: u64 = s["mains"][q].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).sum();
            assert_eq!(n as usize, main.len(), "seed {seed}: P{q}'s main in the spec line");
            // the spec line's list is the pairing's 75: its names are the deck's main and sideboard names
            let d = &spec["metagame"]["decks"][p.decks[q] as usize];
            let mut want: Vec<&str> = main.iter().chain(d["sideboard"].as_array().into_iter().flatten()).map(|x| x.as_str().unwrap()).collect();
            want.sort_unstable();
            want.dedup();
            let mut got: Vec<&str> = s["lists"][q].as_array().unwrap().iter().map(|e| e[0].as_str().unwrap()).collect();
            got.sort_unstable();
            assert_eq!(got, want, "seed {seed}: P{q}'s list is its pairing's 75");
        }
        assert!(g["result"].is_array() || g["aborted"] == true);
    }
    assert!(cells.len() >= 4, "{kind}: only the cells {cells:?}");
    // one spec line per pairing seen
    assert_eq!(spec_lines.len(), cells.len());
}

#[test]
fn every_metagame_game_logs_its_pairing() {
    let decks = [names("izzet_delver_8016541"), names("doomsday_8027957"), names("sneak_show_8014220")];
    let mains: Vec<Value> = decks.iter().map(|d| json!({"name": d["name"], "main": d["main"]})).collect();
    let bo1 = json!({"max_turns": 60, "constructed": true, "metagame": {"decks": mains}});
    let (lines, n) = played(&bo1, 18, "bo1");
    check(&bo1, &lines, "bo1", n);
    let bo3 = json!({"max_turns": 60, "constructed": true, "format": "bo3", "game_episodes": true, "metagame": {"decks": decks}});
    let (lines, n) = played(&bo3, 8, "bo3");
    check(&bo3, &lines, "bo3", n);
    // a fixed constructed best-of-one spec logs nothing (as before)
    let fixed = json!({"max_turns": 60, "constructed": true, "decks": {"p0": decks[0]["main"], "p1": decks[1]["main"]}});
    let (lines, _) = played(&fixed, 2, "fixed");
    assert!(lines.is_empty(), "a fixed best-of-one spec logs no line");
}
