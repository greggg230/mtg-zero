//! `team_draft` through zero-core's deferred hook (ENGINE.md 43, DESIGN.md 5.1): the search
//! evaluates the deferred terminal with the network (rows of no options), the self-play driver
//! parks each event at it with its records, and `resolve` with the results of heuristic-played
//! matches (`draft_bots::play_matches_heuristic`) emits the samples with the real outcome.
use engine::az::draft::{DraftSpec, TeamDraftAz};
use engine::az::draft_bots;
use engine::az::draft_encode::NUM_FEATURES;
use zero_search::driver::{Batch, Driver, DriverCfg, KeyKind, NnSeat, Record, SeatCfg};
use zero_search::game::{SearchGame, MAX_SEATS};

fn cfg(players: usize, events: usize, spec: &str, sims: u32) -> DriverCfg {
    let mut s = NnSeat::eval(0, sims);
    s.selfplay = true;
    s.sims_fast = Some(sims / 2);
    s.p_full = 0.25;
    DriverCfg {
        spec: spec.into(),
        seats: vec![SeatCfg::Nn(s)],
        games: vec![vec![0; players]; events],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: events,
        seed: 11,
        record: true,
        threads: 2,
        groups: 1,
        max_steps: 100_000,
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
    }
}

/// Uniform priors, every seat's value 1/n: a stand-in network. -> rows with no options (the
/// deferred terminal's leaves).
fn answer(d: &mut Driver<TeamDraftAz>, b: &Batch) -> usize {
    let mut pri = Vec::new();
    let mut empty = 0;
    for r in 0..b.b {
        let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
        empty += (k == 0) as usize;
        pri.extend(std::iter::repeat_n(1.0 / k.max(1) as f32, k));
    }
    let mut v = vec![0.0; b.b * MAX_SEATS];
    for r in 0..b.b {
        let n = b.n_players[r] as usize;
        for j in 0..n {
            v[r * MAX_SEATS + j] = 1.0 / n as f32;
        }
    }
    let m = vec![0.0; b.b * MAX_SEATS];
    d.submit(b.id, &pri, &v, &m).unwrap();
    empty
}

fn json_results(res: &[draft_bots::MatchResult]) -> String {
    let v: Vec<serde_json::Value> = res.iter().map(|m| serde_json::json!({"result": m.result, "scores": m.wins})).collect();
    serde_json::Value::Array(v).to_string()
}

/// Events through the driver: parked, resolved with heuristic matches -> (events, parked bytes).
fn events_through_the_driver(players: usize, events: usize, spec: &str, sims: u32) -> (usize, u64) {
    let mut d = Driver::<TeamDraftAz>::new(cfg(players, events, spec, sims)).unwrap();
    let mut empty = 0;
    while let Some(b) = d.next_batch(true).unwrap() {
        empty += answer(&mut d, &b);
    }
    assert!(empty > 0, "the search evaluated deferred terminals");
    assert!(d.take_results().is_empty() && d.take_samples().is_empty(), "nothing finishes before its matches");
    let parked = d.take_deferred();
    assert_eq!(parked.len(), events);
    assert_eq!(d.pending(), events);
    let st = d.stats();
    assert!(st.parked_bytes > 0);
    let t = (players / 2) as f32;
    for (gid, payload) in &parked {
        let v: serde_json::Value = serde_json::from_str(payload).unwrap();
        let ev = d.parked_game(*gid).unwrap().clone();
        assert!(ev.deferred() && ev.to_move().is_none());
        assert_eq!(v["matches"].as_array().unwrap().len(), ev.matches().unwrap().len());
        // a malformed result is refused and the event stays parked
        let before = d.pending();
        assert!(d.resolve(*gid, "[]").is_err() && d.resolve(*gid, "[[0.7, 0.3]]").is_err());
        assert_eq!(d.pending(), before);
        let res = draft_bots::play_matches_heuristic(&ev, 17 + *gid as u64).unwrap();
        let mut truth = ev.clone();
        SearchGame::resolve(&mut truth, &json_results(&res)).unwrap();
        d.resolve(*gid, &json_results(&res)).unwrap();
        let rows = d.take_results();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(row.over && row.gid == *gid);
        let r = truth.result();
        assert_eq!(row.result, r[..players].to_vec());
        assert!(r[..players].iter().all(|&x| x == 0.0 || x == 1.0 / t || x == 0.5 / t));
        let samples: Vec<Record> = d.take_samples();
        assert_eq!(samples.len(), row.samples as usize);
        assert!(!samples.is_empty());
        for s in &samples {
            assert_eq!(s.x.len(), NUM_FEATURES);
            for j in 0..players {
                let seat = (s.player as usize + j) % players;
                assert_eq!(s.z[j], r[seat], "z = the event's real outcome");
                assert_eq!(s.sc[j], truth.margin(seat), "sc = the team's match difference / matches");
            }
            assert!((s.q[..players].iter().sum::<f32>() - 1.0).abs() < 1e-3);
        }
    }
    assert_eq!(d.pending(), 0);
    assert!(d.finished());
    (events, st.parked_bytes)
}

#[test]
fn team_draft_events_park_and_resolve_through_the_driver() {
    let (n, bytes) = events_through_the_driver(4, 2, r#"{"packs": 1, "pack_size": 6}"#, 4);
    eprintln!("2v2, one pack of 6: {n} events parked, {bytes} bytes");
    let (n, bytes) = events_through_the_driver(2, 2, "{}", 4);
    eprintln!("1v1, three packs of 15: {n} events parked, {:.2} MB per event", bytes as f64 / n as f64 / 1e6);
}

/// Parked memory of real-sized events (DESIGN.md 2.3.9: ~185 records of 14,488 features per 4v4
/// event in float16, ~5 MB): `cargo test --release -p engine --test draft_deferred -- --ignored
/// --nocapture`.
#[test]
#[ignore]
fn measure_parked_memory() {
    for players in [4usize, 8] {
        let t0 = std::time::Instant::now();
        let (n, bytes) = events_through_the_driver(players, 4, "{}", 4);
        eprintln!("{}v{}: {n} events, {:.2} MB parked per event ({:.1} s)", players / 2, players / 2, bytes as f64 / n as f64 / 1e6, t0.elapsed().as_secs_f64());
    }
}

#[test]
fn the_hook_maps_the_inherent_methods() {
    let spec = DraftSpec::parse(r#"{"packs": 1, "pack_size": 3}"#).unwrap();
    for players in [2usize, 4, 6, 8] {
        let g = TeamDraftAz::new(&spec, players, 3).unwrap();
        assert_eq!(SearchGame::team_size(&g), players / 2);
        assert!(!SearchGame::deferred(&g));
    }
}
