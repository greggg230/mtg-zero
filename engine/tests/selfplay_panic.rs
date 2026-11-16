//! A self-play game that panics is dropped, not the run (ENGINE.md 172; zero-core docs/search.md 2.18, 2.19), on
//! real MTG games: zero-core's `Driver<MtgAz>` plays the Delver best-of-three spec of the digests
//! (`tests/data/digest_specs.json`: constructed, one episode per game, so a contest crosses episode boundaries) with
//! the test fault `MTGZ_TEST_PANIC=<game seed>:<turn>` armed for one game. Exactly that game is dropped (an aborted
//! row, reason "panic", no samples, one failure with its site in the engine), every other game's row and samples are
//! bit for bit an unarmed run's, and the failure replays the panic (`zero_search::record::replay_failure`); unarmed,
//! the same replay does not panic. One test: the fault is the process's environment.

use engine::az::{MtgAz, MtgSpec};
use zero_search::driver::{Driver, DriverCfg, GameFailure, GameResult, KeyKind, NnSeat, Record, SeatCfg};
use zero_search::game::{SearchGame, MAX_SEATS};

const SEED: u64 = 171;
const GAMES: usize = 8;
const DROP: u32 = 5;
const TURN: u32 = 3;

fn spec() -> String {
    let all: serde_json::Value = serde_json::from_str(include_str!("data/digest_specs.json")).unwrap();
    all["delver_bo3"].to_string()
}

/// FNV-1a over a game's samples (every float's bits, options, policy, targets).
fn samples_hash(rs: &[Record]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut put = |x: u64| {
        for b in x.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    for r in rs {
        put(r.player as u64);
        for (i, v) in r.x.iter().enumerate().filter(|(_, v)| v.to_bits() != 0) {
            put(i as u64);
            put(v.to_bits() as u64);
        }
        r.opts.iter().for_each(|&o| put(o as u64));
        r.pi.iter().chain(&r.z).chain(&r.q).chain(&r.sc).for_each(|v| put(v.to_bits() as u64));
        r.tail.val.iter().for_each(|v| put(v.to_bits() as u64));
        r.ov.iter().for_each(|&(o, t)| {
            put(o as u64);
            put(t as u64)
        });
    }
    h
}

/// One self-play phase of `GAMES` games (2 groups, 2 threads, 4 simulations, an evaluator that is a function of each
/// row alone: uniform priors, value 0.5) -> every game's row with its samples' hash (by gid), the failures, the
/// driver's game_panics.
fn play() -> (Vec<(GameResult, u64)>, Vec<GameFailure>, u64) {
    let mut seat = NnSeat::eval(0, 4);
    seat.selfplay = true;
    let cfg = DriverCfg {
        spec: spec(),
        seats: vec![SeatCfg::Nn(seat)],
        games: vec![vec![0, 0]; GAMES],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: GAMES,
        seed: SEED,
        record: true,
        threads: 2,
        groups: 2,
        max_steps: 60_000,
        rolling: false,
        net: 0,
        queue_only: false,
        move_log: 0,
        aux: false,
        paired: false,
        sparse: true,
        branch: None,
        starts: None,
        max_row_tokens: None,
    };
    let mut d = Driver::<MtgAz>::new(cfg).unwrap();
    let mut rows = Vec::new();
    let mut take = |d: &mut Driver<MtgAz>| {
        let res = d.take_results();
        let mut smp = d.take_samples().into_iter();
        for r in res {
            let mine: Vec<Record> = smp.by_ref().take(r.samples as usize).collect();
            assert_eq!(mine.len(), r.samples as usize, "game {}: its samples", r.gid);
            rows.push((r, samples_hash(&mine)));
        }
        assert!(smp.next().is_none(), "samples beyond the finished games");
    };
    while let Some(b) = d.next_batch(true).unwrap() {
        let mut pri = Vec::with_capacity(b.opt_action.len());
        for r in 0..b.b {
            let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
            pri.extend(std::iter::repeat_n(1.0 / k.max(1) as f32, k));
        }
        let mut v = vec![0.0f32; b.b * MAX_SEATS];
        for r in 0..b.b {
            v[r * MAX_SEATS] = 0.5;
            v[r * MAX_SEATS + 1] = 0.5;
        }
        d.submit(b.id, &pri, &v, &vec![0.0; b.b * MAX_SEATS]).unwrap();
        take(&mut d);
    }
    take(&mut d);
    rows.sort_by_key(|(r, _)| r.gid);
    let st = d.stats();
    (rows, d.take_failures(), st.game_panics)
}

#[test]
fn a_panicking_mtg_game_is_dropped_and_replays() {
    let target = zero_search::record::game_seed(SEED, DROP, false);
    std::env::remove_var("MTGZ_TEST_PANIC");
    let (plain, f0, p0) = play();
    assert!(f0.is_empty() && p0 == 0, "unarmed: no failure");
    assert_eq!(plain.iter().map(|(r, _)| r.gid).collect::<Vec<_>>(), (0..GAMES as u32).collect::<Vec<_>>());
    assert!(plain.iter().all(|(r, _)| r.abort_reason.is_none()));
    std::env::set_var("MTGZ_TEST_PANIC", format!("{target}:{TURN}"));
    let (armed, fails, panics) = play();
    std::env::remove_var("MTGZ_TEST_PANIC");
    assert_eq!(panics, 1, "one game dropped");
    assert_eq!(armed.len(), GAMES, "a row for every game");
    for ((a, ha), (p, hp)) in armed.iter().zip(&plain) {
        if a.gid == DROP {
            assert!(a.aborted && !a.over && a.samples == 0 && a.abort_reason == Some("panic"), "the dropped game's row {a:?}");
            continue;
        }
        assert_eq!(
            (a.gid, a.over, a.aborted, &a.result, a.steps, a.decisions, a.rounds, a.samples, a.episodes.len(), ha),
            (p.gid, p.over, p.aborted, &p.result, p.steps, p.decisions, p.rounds, p.samples, p.episodes.len(), hp),
            "game {}: as the unarmed run",
            a.gid
        );
    }
    assert_eq!(fails.len(), 1);
    let f = &fails[0];
    assert_eq!((f.gid, f.driver_seed, f.game_seed, f.players), (DROP, SEED, target, 2));
    assert!(f.message.contains("[test fault MTGZ_TEST_PANIC=") && f.message.contains(&format!("{target}:{TURN}")), "{}", f.message);
    let site = f.site.clone().unwrap_or_default();
    assert!(site.contains("matches.rs:"), "the panic's site in the engine: {site:?}");
    // the failure replays the panic: its moves (episode boundaries included), then the move or the rollouts
    let sp: MtgSpec = MtgAz::parse_spec(&f.spec).unwrap();
    let j = f.to_json();
    std::env::set_var("MTGZ_TEST_PANIC", format!("{target}:{TURN}"));
    let rep = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| zero_search::record::replay_failure::<MtgAz>(&sp, &j, 16, 1, 20_000)));
    std::env::remove_var("MTGZ_TEST_PANIC");
    let msg = match rep {
        Err(p) => p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default(),
        Ok(r) => panic!("the replay did not panic: {r:?}"),
    };
    assert!(msg.contains("injected test panic"), "{msg}");
    // unarmed, the same replay plays its moves and rollouts without a panic
    let r = zero_search::record::replay_failure::<MtgAz>(&sp, &j, 4, 1, 20_000).unwrap();
    assert_eq!(r.steps, f.steps.len());
    assert!(r.rollout_steps > 0);
}
