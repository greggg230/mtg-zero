//! Measurements of the constructed Delver mirror (ENGINE.md 53.6), `--ignored`:
//!
//! `cargo test --release -p engine --test measure_delver -- --ignored --nocapture --test-threads=1`
//!
//! `MEASURE_GAMES=n` (default 2,000) games of each pairing on the pinned 60s, seats alternating
//! (the seed's chooser decides who plays first; the heuristic always plays first, ENGINE.md
//! 35.4): turns and decisions (two options or more) per game, the mulligan rate, the first
//! player's score, draws, games per second on one thread.

use engine::az::bots::Heuristic;
use engine::az::constructed::Decklist;
use engine::az::{MtgAz, MtgSpec};
use engine::rng::Rng;
use zero_search::game::SearchGame;

fn mirror() -> MtgSpec {
    let names = Decklist::izzet_delver().main_names();
    MtgSpec::parse(&serde_json::json!({"max_turns": 60, "constructed": true, "decks": {"p0": names, "p1": names}}).to_string()).unwrap()
}

#[derive(Clone, Copy, PartialEq)]
enum Who {
    Heuristic,
    Random,
}

struct Stats {
    games: u64,
    turns: u64,
    decisions: u64,
    mulligans: u64,
    hands: u64,
    first_score: f64,
    first_score_sq: f64,
    draws: u64,
    capped: u64,
    seat0_score: f64,
    secs: f64,
}

fn play(sp: &MtgSpec, n: u64, seats: [Who; 2]) -> Stats {
    let mut s = Stats {
        games: 0,
        turns: 0,
        decisions: 0,
        mulligans: 0,
        hands: 0,
        first_score: 0.0,
        first_score_sq: 0.0,
        draws: 0,
        capped: 0,
        seat0_score: 0.0,
        secs: 0.0,
    };
    let t0 = std::time::Instant::now();
    let mut keys = Vec::new();
    let mut legal = Vec::new();
    for seed in 0..n {
        let mut g = MtgAz::new_game(sp, 2, seed).unwrap();
        let mut hs = [Heuristic::new(seed * 2), Heuristic::new(seed * 2 + 1)];
        let mut rng = Rng::new(seed ^ 0x5EED);
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal);
            if legal.len() > 1 {
                s.decisions += 1;
            }
            let a = match seats[p] {
                Who::Heuristic => hs[p].choose(&g.g, &legal),
                Who::Random => legal[rng.below(legal.len() as u32) as usize],
            };
            g.step(a, &mut keys).unwrap();
        }
        let r = g.result();
        let first = g.g.turn().starting as usize;
        s.games += 1;
        s.turns += g.g.turn().number as u64;
        for p in 0..2u8 {
            s.hands += 1;
            s.mulligans += (g.g.player(p).mulligans > 0) as u64;
        }
        s.first_score += r[first] as f64;
        s.first_score_sq += (r[first] as f64).powi(2);
        s.seat0_score += r[0] as f64;
        if r[0] == r[1] {
            s.draws += 1;
            s.capped += (g.g.turn().number >= sp.max_turns) as u64;
        }
    }
    s.secs = t0.elapsed().as_secs_f64();
    s
}

fn report(name: &str, s: &Stats) {
    let n = s.games as f64;
    let m = s.first_score / n;
    let se = ((s.first_score_sq / n - m * m) / n).sqrt();
    println!(
        "{name}: {} games, {:.1} turns, {:.1} decisions per game, mulligan rate {:.1}% of hands, first player {:.1}% +- {:.1}, seat 0 {:.1}%, draws {} ({} at the turn cap), {:.0} games/s ({:.0} decisions/s, one thread)",
        s.games,
        s.turns as f64 / n,
        s.decisions as f64 / n,
        100.0 * s.mulligans as f64 / s.hands as f64,
        100.0 * m,
        100.0 * se,
        100.0 * s.seat0_score / n,
        s.draws,
        s.capped,
        n / s.secs,
        s.decisions as f64 / s.secs
    );
}

#[test]
#[ignore]
fn measure_the_mirror() {
    let n: u64 = std::env::var("MEASURE_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(2000);
    let sp = mirror();
    report("heuristic vs heuristic", &play(&sp, n, [Who::Heuristic, Who::Heuristic]));
    report("heuristic (seat 0) vs random", &play(&sp, n, [Who::Heuristic, Who::Random]));
    report("random vs heuristic (seat 1)", &play(&sp, n, [Who::Random, Who::Heuristic]));
    report("random vs random", &play(&sp, n, [Who::Random, Who::Random]));
}
