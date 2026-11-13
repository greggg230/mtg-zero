//! The zero-core driver on the live Delver best-of-three spec with an instantaneous evaluator: the
//! search threads' capacity alone, and a hash of everything the driver produces (ENGINE.md 145).
//!
//!   cargo test --release -p engine --test driver_bench -- --ignored --nocapture drv
//!   MAXBATCH=40000 CONC=48 GROUPS=2 THREADS=8 cargo test ... drv     (the fixed-work hash)
//!   cargo test --release -p engine --test driver_bench -- --ignored --nocapture padding
//!
//! `drv`: the run's self-play seat (64 / 16 / 4 simulations, the cq target, eval cache 8192, root-view
//! leaves with view samples 0.5, hand tilt 1 + deck tilt 1 with mix 0.25, resignation), sparse batches,
//! the auxiliary targets, rolling. With `MAXBATCH` > 0 it stops after that many batches and hashes every
//! batch's sparse rows, options, tails, overrides and game ids, every sample (its non-zero features,
//! targets, options, overrides) and every result: the behaviour check for what `digest.rs` cannot see
//! (the driver, the belief and deck deals, the constructed prior, the eval cache, the records). The hash
//! does not depend on the thread count. Without it the run is timed (`SECS`, a steady window after
//! `WARM` seconds). It prints the process's peak working set (Windows).
//!
//! `padding`: the token graphs' padding of the same batches (rows to 64, tokens to the longest row's,
//! rounded to 16) and of 2-4 length-sorted chunks (zero-core `infer.chunk_rows`' cost model), the
//! census of `infer_token_chunks` (zero-core docs/az.md 43.11).
//!
//! Settings by environment: THREADS (8), GROUPS (2), CONC (768), SECS (60), WARM (20), SEED (11),
//! SIMS / SIMS_FAST / SIMS_MINOR (64 / 16 / 4), CACHE (8192), ROOTVIEW / TILT / RESIGN (1 = on),
//! SPEC (a spec JSON file; default `data/driver_bench_spec.json`).
//!
//! The binary runs on mimalloc, the allocator of the Python module `mtgz._engine` (py/Cargo.toml): on
//! the Windows heap the same driver run is ~40% slower and its allocation-heavy parts (records,
//! deals) several times slower (the 2026-11-12 review, perf.md bug 3).
use engine::az::identity;
use engine::az::MtgAz;
use std::hint::black_box;
use std::time::Instant;
use zero_search::driver::{Driver, DriverCfg, KeyKind, LeafView, NnSeat, PolicyTarget, Record, SeatCfg};
use zero_search::game::SearchGame;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn spec_json() -> String {
    match std::env::var("SPEC") {
        Ok(p) => std::fs::read_to_string(p).unwrap(),
        Err(_) => include_str!("data/driver_bench_spec.json").to_string(),
    }
}

fn env(k: &str, d: u64) -> u64 {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

/// The process's peak working set in bytes (None off Windows).
#[cfg(windows)]
fn peak_ws() -> Option<usize> {
    #[repr(C)]
    #[derive(Default)]
    struct Pmc {
        cb: u32,
        faults: u32,
        peak_ws: usize,
        ws: usize,
        quota: [usize; 4],
        pagefile: usize,
        peak_pagefile: usize,
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(h: isize, p: *mut Pmc, cb: u32) -> i32;
    }
    let mut p = Pmc { cb: std::mem::size_of::<Pmc>() as u32, ..Pmc::default() };
    // SAFETY: a plain query of this process into a struct of the documented layout (PROCESS_MEMORY_COUNTERS)
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut p, p.cb) };
    (ok != 0).then_some(p.peak_ws)
}

#[cfg(not(windows))]
fn peak_ws() -> Option<usize> {
    None
}

fn seat() -> NnSeat {
    let mut s = NnSeat::eval(0, env("SIMS", 64) as u32);
    s.selfplay = true;
    s.sims_fast = Some(env("SIMS_FAST", 16) as u32);
    s.p_full = 0.25;
    s.sims_minor = Some(env("SIMS_MINOR", 4) as u32);
    s.temp_moves = 60;
    s.policy_target = PolicyTarget::CompletedQ { c_visit: 50.0, c_scale: 0.3, prior_w: 1.0 };
    s.eval_cache = env("CACHE", 8192) as u32;
    if env("ROOTVIEW", 1) == 1 {
        s.leaf_value_view = LeafView::Root;
        s.view_samples = 0.5;
    }
    if env("TILT", 1) == 1 {
        s.belief_tilt = 1.0;
        s.belief_mix = 0.25;
        s.belief_tilts = vec![("deck".to_string(), 1.0)];
    }
    if env("RESIGN", 1) == 1 {
        s.resign_t = 0.02;
        s.resign_k = 3;
        s.resign_noresign = 0.1;
    }
    s
}

fn cfg(threads: usize, groups: usize, conc: usize) -> DriverCfg {
    DriverCfg {
        spec: spec_json(),
        seats: vec![SeatCfg::Nn(seat())],
        games: vec![vec![0, 0]],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: conc,
        seed: env("SEED", 11),
        record: true,
        threads,
        groups,
        max_steps: 60_000,
        rolling: true,
        net: 0,
        queue_only: false,
        move_log: 0,
        aux: true,
        paired: false,
        sparse: true,
        branch: None,
        starts: None,
        max_row_tokens: None,
    }
}

/// The instantaneous evaluator's answer to a batch: a peaked prior as a trained network's (the first option
/// 0.85), value 0.5 / 0.5, margins 0, a uniform belief.
fn answer(d: &mut Driver<MtgAz>, b: &zero_search::driver::Batch) {
    let mut pri = Vec::with_capacity(b.opt_action.len());
    for r in 0..b.b {
        let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
        for j in 0..k {
            pri.push(if k == 1 {
                1.0
            } else if j == 0 {
                0.85
            } else {
                0.15 / (k - 1) as f32
            });
        }
    }
    let mut v = vec![0.0f32; b.b * 8];
    for r in 0..b.b {
        v[r * 8] = 0.5;
        v[r * 8 + 1] = 0.5;
    }
    let nb = b.extra.iter().filter(|&&e| e == zero_search::driver::ROW_BELIEF).count();
    let aux = vec![1.0 / identity::D as f32; nb * MtgAz::aux_width()];
    d.submit_aux(b.id, &pri, &v, &vec![0.0; b.b * 8], &aux).unwrap();
}

fn mix(h: &mut u64, v: u64) {
    *h = h.wrapping_mul(0x100000001b3) ^ v;
}

/// A record's features into the hash: each non-zero float (its bits not 0) as (index << 32 | bits), in index order.
fn hash_features(h: &mut u64, r: &Record) {
    for (i, &v) in r.x.iter().enumerate() {
        if v.to_bits() != 0 {
            mix(h, (i as u64) << 32 | v.to_bits() as u64);
        }
    }
}

#[test]
#[ignore]
fn drv() {
    let threads = env("THREADS", 8) as usize;
    let groups = env("GROUPS", 2) as usize;
    let conc = env("CONC", 768) as usize;
    let secs = env("SECS", 60) as f64;
    let warm = env("WARM", 20) as f64;
    let maxb = env("MAXBATCH", 0);
    let mut d = Driver::<MtgAz>::new(cfg(threads, groups, conc)).unwrap();
    d.resume(1_000_000, 0).unwrap();
    let t0 = Instant::now();
    let (mut batches, mut rows, mut nnz, mut ttok, mut opts) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut warm_leaves, mut warm_t, mut warm_busy, mut warm_sims) = (0u64, 0.0f64, 0.0f64, 0u64);
    let (mut hash, mut nrec, mut nres) = (0u64, 0u64, 0u64);
    while maxb > 0 || t0.elapsed().as_secs_f64() < secs {
        let Some(b) = d.next_batch(true).unwrap() else { break };
        if maxb > 0 {
            for &i in &b.xs_idx {
                mix(&mut hash, i as u64);
            }
            for &v in &b.xs_val {
                mix(&mut hash, v.to_bits() as u64);
            }
            for &a in &b.opt_action {
                mix(&mut hash, a as u64);
            }
            for &v in &b.t_val {
                mix(&mut hash, v.to_bits() as u64);
            }
            for &s in &b.t_slot {
                mix(&mut hash, s as u64);
            }
            for &t in &b.ov_tok {
                mix(&mut hash, t as u64);
            }
            for &g in &b.gid {
                mix(&mut hash, g as u64);
            }
        }
        answer(&mut d, &b);
        batches += 1;
        rows += b.b as u64;
        nnz += b.xs_val.len() as u64;
        ttok += b.t_seg.len() as u64;
        opts += b.opt_action.len() as u64;
        if maxb > 0 {
            for r in d.take_samples() {
                nrec += 1;
                hash_features(&mut hash, &r);
                for &v in r.pi.iter().chain(&r.aux).chain(&r.tail.val).chain(&r.z).chain(&r.q) {
                    mix(&mut hash, v.to_bits() as u64);
                }
                for &o in &r.opts {
                    mix(&mut hash, o as u64);
                }
                for &(o, t) in &r.ov {
                    mix(&mut hash, (o as u64) ^ ((t as u64) << 8));
                }
            }
            for r in d.take_results() {
                nres += 1;
                for &v in r.scores.iter() {
                    mix(&mut hash, v.to_bits() as u64);
                }
                mix(&mut hash, r.gid as u64 ^ (r.steps << 32));
            }
            if batches >= maxb {
                break;
            }
        } else {
            black_box(d.take_samples());
            black_box(d.take_results());
        }
        if maxb == 0 && warm_t == 0.0 && t0.elapsed().as_secs_f64() >= warm {
            let st = d.stats();
            (warm_leaves, warm_sims, warm_busy, warm_t) = (st.leaves, st.sims, st.busy_s, t0.elapsed().as_secs_f64());
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    let st = d.stats();
    match peak_ws() {
        Some(b) => println!("  peak working set {:.2} GB", b as f64 / 1e9),
        None => println!("  peak working set n/a"),
    }
    if warm_t > 0.0 {
        let (dl, ds) = (st.leaves - warm_leaves, st.sims - warm_sims);
        println!(
            "  steady [{warm_t:.0}s, {secs:.0}s]: {:.0} leaves/s, {:.0} sims/s, {:.1} thread-us per leaf, {:.1} per sim",
            dl as f64 / (secs - warm_t),
            ds as f64 / (secs - warm_t),
            (st.busy_s - warm_busy) * 1e6 / dl.max(1) as f64,
            (st.busy_s - warm_busy) * 1e6 / ds.max(1) as f64,
        );
    }
    let hash = if maxb > 0 { format!("{hash:016x}") } else { "- (MAXBATCH=0)".into() };
    println!(
        "drv threads={threads} groups={groups} conc={conc}: {secs:.1}s, leaves {} = {:.0}/s, sims {}, batches {batches} (avg rows {:.0}, nnz/row {:.0}, tail tok/row {:.2}, opts/row {:.2}), view rows {}, belief rows {}, cache hits {} leaves {} partial {}, games started {} finished {}, recorded {}, minor {}; busy_s {:.1} ({:.1} us/sim) assemble_s {:.1} wait_s {:.1}; samples {nrec} results {nres} hash {hash}",
        st.leaves,
        st.leaves as f64 / secs,
        st.sims,
        rows as f64 / batches.max(1) as f64,
        nnz as f64 / rows.max(1) as f64,
        ttok as f64 / rows.max(1) as f64,
        opts as f64 / rows.max(1) as f64,
        st.view_rows,
        st.belief_rows,
        st.cache_hits,
        st.cache_leaves,
        st.cache_partial,
        st.games_started,
        st.games_finished,
        st.recorded,
        st.minor,
        st.busy_s,
        st.busy_s * 1e6 / st.sims.max(1) as f64,
        st.assemble_s,
        st.wait_s,
    );
}

/// Padding of the token network's batches (module docs): per self-play batch each row's tokens (the global
/// token, the head slots present, the tail tokens), the batch padded as zero-core's CUDA graphs pad it, and
/// the same batch cut into at most 2-4 length-sorted chunks by `chunk_rows`' cost model.
#[test]
#[ignore]
fn padding() {
    let lay: serde_json::Value = serde_json::from_str(&engine::az::encode::layout_json(engine::db::CardDb::cube())).unwrap();
    let f = MtgAz::NUM_FEATURES;
    let mut flag = vec![false; f];
    let mut always = 1usize; // the global token
    for s in lay["segments"].as_array().unwrap() {
        let (Some(o), Some(k), Some(w)) = (s["offset"].as_u64(), s["count"].as_u64(), s["width"].as_u64()) else {
            continue; // (a tail segment: no head slots)
        };
        let (o, k, w) = (o as usize, k as usize, w as usize);
        match s["present"].as_u64() {
            Some(p) => {
                for j in 0..k {
                    flag[o + j * w + p as usize] = true;
                }
            }
            None => always += k,
        }
    }
    let conc = env("CONC", 1536) as usize;
    let groups = env("GROUPS", 4) as usize;
    let mut d = Driver::<MtgAz>::new(cfg(env("THREADS", 8) as usize, groups, conc)).unwrap();
    d.resume(1_000_000, 0).unwrap();
    let t0 = Instant::now();
    let warm = env("WARM", 40) as f64;
    let nb = env("BATCHES", 600);
    let cost = |bp: usize, tp: usize| (bp * tp) as f64 * (1.0 + tp as f64 / 256.0) + 4096.0;
    let up = |x: usize, s: usize| x.div_ceil(s) * s;
    // totals per chunk count 1..=4: (token slots, attention slots x T, graphs)
    let mut tot = [(0f64, 0f64, 0u64); 4];
    let (mut rows, mut toks, mut batches, mut long, mut tmax_sum, mut tailtok) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut hist = vec![0u64; 257];
    while batches < nb {
        let Some(b) = d.next_batch(true).unwrap() else { break };
        if t0.elapsed().as_secs_f64() >= warm {
            let mut cnt: Vec<usize> = (0..b.b)
                .map(|r| {
                    let (lo, hi) = (b.xs_off[r] as usize, b.xs_off[r + 1] as usize);
                    always + b.xs_idx[lo..hi].iter().filter(|&&i| flag[i as usize]).count() + (b.t_off[r + 1] - b.t_off[r]) as usize
                })
                .collect();
            tailtok += b.t_seg.len() as u64;
            long += cnt.iter().filter(|&&c| c > 128).count() as u64;
            cnt.retain(|&c| c <= 128);
            cnt.sort_unstable();
            let n = cnt.len();
            if n > 0 {
                rows += n as u64;
                toks += cnt.iter().sum::<usize>() as u64;
                for &c in &cnt {
                    hist[c.min(256)] += 1;
                }
                tmax_sum += up(cnt[n - 1], 16) as u64;
                batches += 1;
                let cuts: Vec<usize> = (1..).map(|i| i * 64).take_while(|&c| c < n).collect();
                let piece = |a: usize, e: usize| (up(e - a, 64), up(cnt[e - 1], 16));
                let value = |e: &[usize]| -> f64 {
                    e.windows(2)
                        .map(|w| {
                            let (bp, tp) = piece(w[0], w[1]);
                            cost(bp, tp)
                        })
                        .sum()
                };
                for k in 1..=4usize {
                    // the best cut into at most k pieces by the cost model (brute force, as chunk_rows)
                    let mut best = (cost(up(n, 64), up(cnt[n - 1], 16)), vec![0, n]);
                    let mut cands: Vec<Vec<usize>> = Vec::new();
                    let m = cuts.len();
                    for i in 0..m {
                        if k >= 2 {
                            cands.push(vec![0, cuts[i], n]);
                        }
                        for j in i + 1..m {
                            if k >= 3 {
                                cands.push(vec![0, cuts[i], cuts[j], n]);
                            }
                            for l in j + 1..m {
                                if k >= 4 {
                                    cands.push(vec![0, cuts[i], cuts[j], cuts[l], n]);
                                }
                            }
                        }
                    }
                    for e in cands {
                        let v = value(&e);
                        if v < best.0 {
                            best = (v, e);
                        }
                    }
                    for w2 in best.1.windows(2) {
                        let (bp, tp) = piece(w2[0], w2[1]);
                        tot[k - 1].0 += (bp * tp) as f64;
                        tot[k - 1].1 += (bp * tp * tp) as f64;
                        tot[k - 1].2 += 1;
                    }
                }
            }
        }
        answer(&mut d, &b);
        black_box(d.take_samples());
        black_box(d.take_results());
    }
    let pct = |q: f64| {
        let mut acc = 0u64;
        for (t, &c) in hist.iter().enumerate() {
            acc += c;
            if acc as f64 >= q * rows as f64 {
                return t;
            }
        }
        256
    };
    println!(
        "padding conc={conc} groups={groups}: {batches} batches, {:.0} rows a batch, {:.2} tokens a row (tail {:.2}; p50 {} p90 {} p99 {} max {}), rows over 128 tokens {long}; the whole batch's padded T {:.1} on average",
        rows as f64 / batches.max(1) as f64,
        toks as f64 / rows.max(1) as f64,
        tailtok as f64 / rows.max(1) as f64,
        pct(0.5),
        pct(0.9),
        pct(0.99),
        pct(1.0),
        tmax_sum as f64 / batches.max(1) as f64
    );
    for k in 0..4 {
        println!(
            "  at most {} chunk(s): {:.2} graphs a batch; token slots {:.1}% real ({:.3} of the whole batch's); attention B*T^2 {:.3} of the whole batch's",
            k + 1,
            tot[k].2 as f64 / batches.max(1) as f64,
            toks as f64 * 100.0 / tot[k].0.max(1.0),
            tot[k].0 / tot[0].0.max(1.0),
            tot[k].1 / tot[0].1.max(1.0)
        );
    }
}
