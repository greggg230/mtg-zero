//! ENGINE.md 230.7: the search impact of the 601.2g step (`cast_mana.rs`) - decisions per game and options per
//! decision of random play in the fuzz's games of a pool (`FUZZ_POOL`, `fuzz::games::new_game`; the cube without
//! one), and the step's own (`CastMana` decisions, their options). Ignored; run on a branch and on its base with
//! the same seeds:
//! `FUZZ_POOL=delver MEASURE_GAMES=400 cargo test --release -p engine --test misc_suite measure_cast_mana:: -- --ignored --nocapture`.
//! (The kind is matched by name, so the file builds on an engine without the step.)

#[test]
#[ignore]
fn decisions_and_options_per_game() {
    let n: u64 = std::env::var("MEASURE_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let (mut dec, mut opts, mut cm_dec, mut cm_opts, mut multi, mut multi_opts) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let t0 = std::time::Instant::now();
    let mut legal = Vec::new();
    let mut chance = Vec::new();
    for seed in 0..n {
        let mut g = engine::fuzz::games::new_game(seed);
        let mut r = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut steps = 0u32;
        while !g.is_over() && steps < 100_000 {
            g.legal_actions(&mut legal);
            dec += 1;
            opts += legal.len() as u64;
            if legal.len() > 1 {
                multi += 1;
                multi_opts += legal.len() as u64;
            }
            if format!("{:?}", g.kind()) == "Some(CastMana)" {
                cm_dec += 1;
                cm_opts += legal.len() as u64;
            }
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            let a = legal[(r % legal.len() as u64) as usize];
            g.apply(a, &mut chance).expect("a legal action");
            steps += 1;
        }
    }
    let pool = std::env::var("FUZZ_POOL").unwrap_or_else(|_| "cube".into());
    let f = |a: u64, b: u64| if b == 0 { 0.0 } else { a as f64 / b as f64 };
    println!(
        "MEASURE pool={pool} games={n} decisions/game={:.2} options/decision={:.3} decisions with 2+ options/game={:.2} their options={:.3} CastMana/game={:.3} its options={:.3} seconds={:.2} decisions/s={:.0}",
        f(dec, n),
        f(opts, dec),
        f(multi, n),
        f(multi_opts, multi),
        f(cm_dec, n),
        f(cm_opts, cm_dec),
        t0.elapsed().as_secs_f64(),
        dec as f64 / t0.elapsed().as_secs_f64()
    );
}
