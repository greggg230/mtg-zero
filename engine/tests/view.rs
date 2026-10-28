//! The spectator view (`az/view.rs`, zero-core `export_state`): the replay recorder's state at
//! every step. Delver mirror games played by the heuristic and at random: every state is valid
//! JSON with both players' zones, each pending option has words, the log's tail follows the log,
//! and exporting changes nothing (the same game with and without the export and the log).

use engine::az::bots::Heuristic;
use engine::az::constructed::Decklist;
use engine::az::view::VIEW_VERSION;
use engine::az::{MtgAz, MtgSpec};
use engine::rng::Rng;
use serde_json::Value;
use zero_search::game::SearchGame;

fn mirror(log: bool) -> MtgSpec {
    let names = Decklist::izzet_delver().main_names();
    MtgSpec::parse(&serde_json::json!({"max_turns": 60, "constructed": true, "log": log, "decks": {"p0": names, "p1": names}}).to_string())
        .unwrap()
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// One game; `export` = check the view at every step. -> (actions taken, final view).
fn play(spec: &MtgSpec, seed: u64, heuristic: bool, export: bool) -> (Vec<u16>, Value) {
    let mut g = MtgAz::new(spec, seed).unwrap();
    let mut bot = Heuristic::new(seed);
    let mut rng = Rng::new(seed ^ 0x5eed);
    let mut chance = Vec::new();
    let mut taken = Vec::new();
    let mut log_n = 0u64;
    while let Some(p) = g.to_move() {
        let acts = legal(&g);
        if export {
            let v: Value = serde_json::from_str(&g.export_state()).unwrap();
            check(&v, &acts, p, &mut log_n, spec.log);
        }
        let a = if heuristic { bot.choose(&g.g, &acts) } else { acts[rng.below(acts.len() as u32) as usize] };
        taken.push(a);
        g.step(a, &mut chance).unwrap();
        assert!(taken.len() < 20_000, "game {seed} too long");
    }
    (taken, serde_json::from_str(&g.export_state()).unwrap())
}

fn check(v: &Value, acts: &[u16], p: usize, log_n: &mut u64, log: bool) {
    assert_eq!(v["v"], VIEW_VERSION);
    let players = v["players"].as_array().unwrap();
    assert_eq!(players.len(), 2);
    for pl in players {
        for k in ["hand", "graveyard", "exile", "battlefield"] {
            for c in pl[k].as_array().unwrap() {
                assert!(!c["name"].as_str().unwrap().is_empty(), "{k}: {c}");
            }
        }
        // 60 cards between library, hand, graveyard, exile, the battlefield and the stack
        // (tokens aside), each player's own.
        let n = pl["library"].as_u64().unwrap() as usize
            + pl["hand"].as_array().unwrap().len()
            + pl["graveyard"].as_array().unwrap().len()
            + pl["exile"].as_array().unwrap().len();
        assert!(n <= 60, "{n} cards outside the battlefield");
    }
    let d = &v["decision"];
    assert_eq!(d["player"].as_u64().unwrap() as usize, p);
    let opts = d["options"].as_array().unwrap();
    let ids: Vec<u16> = opts.iter().map(|o| o["a"].as_u64().unwrap() as u16).collect();
    assert_eq!(ids, acts, "{d}");
    for o in opts {
        let t = o["t"].as_str().unwrap();
        assert!(!t.is_empty() && !t.contains("{:?}"), "{d}");
    }
    let n = v["log_n"].as_u64().unwrap();
    assert!(n >= *log_n, "the log shrank");
    if !log {
        assert_eq!(n, 0);
    }
    assert_eq!(v["log"].as_array().unwrap().len() as u64, n.min(40));
    *log_n = n;
}

#[test]
fn every_state_of_mirror_games() {
    for seed in 0..6 {
        let (_, end) = play(&mirror(true), seed, seed % 2 == 0, true);
        assert_eq!(end["over"], true);
        assert!(end["log_n"].as_u64().unwrap() > 50, "seed {seed}: {}", end["log_n"]);
        assert!(end["decision"].is_null());
    }
}

#[test]
fn exporting_and_the_log_change_nothing() {
    for seed in [3, 11] {
        let (a, _) = play(&mirror(false), seed, true, false);
        let (b, _) = play(&mirror(true), seed, true, true);
        assert_eq!(a, b, "seed {seed}");
    }
}

/// `VIEW_SAMPLE=path cargo test --test view sample -- --ignored`: one heuristic game's views,
/// one JSON per line (for the viewer's development).
#[test]
#[ignore]
fn sample() {
    let path = std::env::var("VIEW_SAMPLE").unwrap_or_else(|_| "view_sample.jsonl".into());
    let spec = mirror(true);
    let mut g = MtgAz::new(&spec, 7).unwrap();
    let mut bot = Heuristic::new(7);
    let mut chance = Vec::new();
    let mut out = String::new();
    while g.to_move().is_some() {
        out += &g.export_state();
        out.push('\n');
        let a = bot.choose(&g.g, &legal(&g));
        g.step(a, &mut chance).unwrap();
    }
    out += &g.export_state();
    std::fs::write(path, out).unwrap();
}
