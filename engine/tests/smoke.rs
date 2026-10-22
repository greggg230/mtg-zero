mod common;

use engine::rng::Rng;
use engine::{Game, GameSpec};

#[test]
fn one_random_game() {
    let db = common::db();
    let mut rng = Rng::new(7);
    let d0 = common::random_deck(db, &mut rng);
    let d1 = common::random_deck(db, &mut rng);
    let spec = GameSpec { log: true, ..GameSpec::default() };
    let mut g = Game::new(db, spec, [&d0, &d1], 42).unwrap();
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    let mut n = 0;
    while !g.is_over() {
        g.legal_actions(&mut legal);
        let a = legal[rng.below(legal.len() as u32) as usize];
        g.apply(a, &mut keys).unwrap();
        g.check_invariants().unwrap();
        n += 1;
    }
    for l in g.log_lines().iter().take(300) {
        println!("{l}");
    }
    println!("decisions {n} outcome {:?} turn {}", g.outcome(), g.turn().number);
}
