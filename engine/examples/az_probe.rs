//! Quick probe of the `mtg` zero-core game: bot vs bot win rates, game length.
use engine::az::{MtgAz, MtgSpec, decks};
use engine::db::CardDb;
use zero_search::game::{SearchGame, make_bot};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let a = args.get(1).map(|s| s.as_str()).unwrap_or("heuristic");
    let b = args.get(2).map(|s| s.as_str()).unwrap_or("random");
    let n: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
    let spec_json = args.get(4).cloned().unwrap_or_default();
    let spec = MtgSpec::parse(&spec_json).unwrap();
    let db = CardDb::cube();
    let mut rng = engine::rng::Rng::new(5);
    for _ in 0..2 {
        let d = decks::generate(db, &mut rng, &spec.gen);
        let names: Vec<&str> = d.iter().map(|&x| db.def(x).name.as_str()).collect();
        println!("deck: {}", names.join(", "));
    }
    let (mut wa, mut draws, mut turns, mut decisions, mut multi) = (0.0f64, 0, 0u64, 0u64, 0u64);
    let t0 = std::time::Instant::now();
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    for s in 0..n {
        let mut g = MtgAz::new_game(&spec, 2, s * 7919 + 1).unwrap();
        let aseat = (s % 2) as usize;
        let mut bots = [make_bot::<MtgAz>(&spec, a, s).unwrap(), make_bot::<MtgAz>(&spec, b, s + 1).unwrap()];
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal);
            decisions += 1;
            let act = if legal.len() == 1 { legal[0] } else {
                multi += 1;
                let who = if p == aseat { 0 } else { 1 };
                bots[who].choose(&g, &legal)
            };
            keys.clear();
            g.step(act, &mut keys).unwrap();
        }
        let r = g.result();
        wa += r[aseat] as f64;
        if r[0] == 0.5 { draws += 1; }
        turns += g.rounds() as u64;
    }
    let dt = t0.elapsed().as_secs_f64();
    println!("{a} vs {b}: {:.1}% ({} games, draws {draws}), turns/game {:.1}, decisions/game {:.1} (multi {:.1}), {:.0} games/s",
        100.0 * wa / n as f64, n, turns as f64 / n as f64, decisions as f64 / n as f64, multi as f64 / n as f64, n as f64 / dt);
}
