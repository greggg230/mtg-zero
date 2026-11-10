//! The heuristic bot (ENGINE.md 21.4): it answers every decision kind with a
//! legal id and every game ends (a loop of optional actions ends when it
//! comes back to a state, `engine/src/loops.rs`), it uses loyalty and
//! other activated abilities in the situations its policy names, and its
//! strength against the policy before activated abilities (`HeuristicV1`)
//! and random. The long measurements are `#[ignore]`d:
//! `cargo test --release -p engine --test bots -- --ignored --nocapture`
//! (`BOTS_GAMES=n` sets the number of deck pairs). The Edge of Eternities
//! yardsticks (ENGINE.md 103): `eoe_yardsticks`, `eoe_mechanics`,
//! `eoe_inventory`, `eoe_trace` (`BOTS_DECKS` cube, eoe or eoe2c).

mod common;

use common::db;
use engine::az::bots::{Heuristic, HeuristicV1};
use engine::az::{MtgAz, MtgSpec};
use engine::db::{types, ActEffect, ActivatedDef, CardDb, Counter};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use std::collections::BTreeMap;
use zero_search::game::SearchGame;

// ---- players ---------------------------------------------------------------

enum Bot {
    New(Heuristic),
    Old(HeuristicV1),
    Random(Rng),
    /// The new bot half the time, else a random legal id (odd states).
    Mixed(Heuristic, Rng),
}

impl Bot {
    fn new(kind: &str, seed: u64) -> Bot {
        match kind {
            "new" => Bot::New(Heuristic::new(seed)),
            // The M7c policy, before ENGINE.md 103's rules.
            "m7c" => Bot::New(Heuristic::m7c(seed)),
            "old" => Bot::Old(HeuristicV1::new(seed)),
            "random" => Bot::Random(Rng::new(seed ^ 0x5EED)),
            "mixed" => Bot::Mixed(Heuristic::new(seed), Rng::new(seed ^ 0xA11)),
            _ => panic!("{kind}"),
        }
    }

    fn choose(&mut self, g: &Game, legal: &[u16]) -> u16 {
        match self {
            Bot::New(h) => h.choose(g, legal),
            Bot::Old(h) => h.choose(g, legal),
            Bot::Random(r) => legal[r.below(legal.len() as u32) as usize],
            Bot::Mixed(h, r) => {
                let a = h.choose(g, legal);
                assert!(legal.contains(&a), "the bot answered {a} ({legal:?})");
                if r.below(2) == 0 {
                    a
                } else {
                    legal[r.below(legal.len() as u32) as usize]
                }
            }
        }
    }
}

struct Played {
    result: [f32; 2],
    /// A draw (the turn cap, or both players lost at once).
    capped: bool,
}

/// One game: `kinds[s]` plays seat `s`. With `every`, the bots answer every
/// decision (single-option ones included) and each answer is checked; else
/// only decisions with two or more options, as zero-core's driver asks them.
/// Kinds of decisions the first bot answered are counted in `seen`.
fn play(spec: &MtgSpec, seed: u64, kinds: [&str; 2], every: bool, seen: &mut BTreeMap<String, u64>) -> Played {
    let mut g = MtgAz::new_game(spec, 2, seed).unwrap();
    let mut bots = [Bot::new(kinds[0], seed * 2 + 1), Bot::new(kinds[1], seed * 2 + 2)];
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    let mut steps = 0u32;
    while let Some(p) = g.to_move() {
        g.legal_actions(&mut legal);
        let a = if legal.len() == 1 && !every {
            legal[0]
        } else {
            let a = bots[p as usize].choose(&g.g, &legal);
            assert!(legal.contains(&a), "seed {seed}: {} answered {a} to {:?} ({legal:?})", kinds[p as usize], g.g.pending_decision().map(|d| d.kind));
            if p == 0 {
                let k = format!("{:?}", g.g.pending_decision().unwrap().kind);
                *seen.entry(k).or_default() += 1;
            }
            a
        };
        keys.clear();
        g.step(a, &mut keys).unwrap();
        steps += 1;
        assert!(steps < 200_000, "seed {seed} ({kinds:?}): the game does not end");
    }
    Played { result: [g.result()[0], g.result()[1]], capped: g.g.outcome().is_some_and(|o| o.winner.is_none()) }
}

fn spec() -> MtgSpec {
    MtgSpec { max_turns: 60, ..MtgSpec::default() }
}

fn games(n: u64) -> u64 {
    std::env::var("BOTS_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(n)
}

/// A non-mana activated ability (or one only at priority) of a card: of the
/// permanent, from hand or from a graveyard.
fn has_activated(d: &engine::db::CardDef) -> bool {
    let at_priority = |a: &ActivatedDef| a.mana().is_none() || a.instant_only;
    d.activated.iter().any(at_priority) || d.hand_abilities.iter().any(at_priority) || !d.gy_abilities.is_empty()
}

/// (a deck has a planeswalker, the number of nonland cards with an
/// activated ability in the two decks; nearly every deck has a land with one,
/// a fetch land).
fn deck_traits(spec: &MtgSpec, seed: u64) -> (bool, usize) {
    let cube = CardDb::cube();
    let decks = spec.decks(seed);
    let all = || decks.iter().flatten().map(|&x| cube.def(x));
    (all().any(|d| d.is(types::PLANESWALKER)), all().filter(|d| !d.is(types::LAND) && has_activated(d)).count())
}

/// The cards with an X in their mana cost (`x_cards`).
#[test]
#[ignore]
fn x_cards() {
    let cube = CardDb::cube();
    for id in 0..cube.len() as u16 {
        if cube.def(id).x_pips > 0 {
            println!("{}", cube.def(id).name);
        }
    }
}

// ---- always legal, every game ends ------------------------------------------

#[test]
fn always_legal_and_every_game_ends() {
    let sp = spec();
    let mut seen = BTreeMap::new();
    for s in 0..8u64 {
        for kinds in [["new", "random"], ["new", "old"], ["new", "new"], ["random", "new"], ["mixed", "mixed"]] {
            play(&sp, 7000 + s, kinds, true, &mut seen);
        }
    }
    assert!(seen.contains_key("Priority") && seen.contains_key("Target"), "{seen:?}");
}

/// The registered zero-core bot `heuristic` is this one: legal, and it beats
/// random (as `tests/az.rs` checks, on other seeds).
#[test]
fn registered_bot_beats_random() {
    let sp = spec();
    // (200 games: 60 were too few to hold 0.85 once the pool changed, M7c
    // tail: 0.833 on the first 60, 0.895 on 200.)
    let (mut score, n) = (0.0, 200u64);
    let mut legal = Vec::new();
    for s in 0..n {
        let mut g = MtgAz::new_game(&sp, 2, 9000 + s).unwrap();
        let hseat = (s % 2) as usize;
        let mut bots = [
            zero_search::game::make_bot::<MtgAz>(&sp, "heuristic", s).unwrap(),
            zero_search::game::make_bot::<MtgAz>(&sp, "random", s).unwrap(),
        ];
        let mut keys = Vec::new();
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal);
            let a = bots[(p as usize != hseat) as usize].choose(&g, &legal);
            assert!(legal.contains(&a));
            g.step(a, &mut keys).unwrap();
        }
        score += g.result()[hseat];
    }
    assert!(score / n as f32 > 0.85, "heuristic vs random {}", score / n as f32);
}

/// A few thousand games, the bots asked at every decision: no illegal id, no
/// endless game; prints the decision kinds the new bot answered.
#[test]
#[ignore]
fn always_legal_and_every_game_ends_long() {
    let sp = spec();
    let n = games(1000);
    let mut seen = BTreeMap::new();
    let t0 = std::time::Instant::now();
    let mut total = 0;
    for s in 0..n {
        for kinds in [["new", "random"], ["new", "old"], ["new", "new"], ["mixed", "mixed"]] {
            play(&sp, 100_000 + s, kinds, true, &mut seen);
            total += 1;
        }
    }
    println!("{total} games in {:.1} s, all legal and ended", t0.elapsed().as_secs_f64());
    println!("decision kinds the new bot answered ({}): {seen:?}", seen.len());
}

// ---- strength ---------------------------------------------------------------

#[derive(Default)]
struct Tally {
    pairs: Vec<f64>,
    draws: u64,
}

impl Tally {
    fn add(&mut self, a: &Played, seat_a: usize, b: &Played, seat_b: usize) {
        self.pairs.push((a.result[seat_a] + b.result[seat_b]) as f64 / 2.0);
        self.draws += a.capped as u64 + b.capped as u64;
    }

    /// Mean score per game, its standard error over deck pairs (each pair is
    /// one deck pair played both ways), games, draws.
    fn line(&self) -> String {
        let n = self.pairs.len() as f64;
        if n == 0.0 {
            return "no games".into();
        }
        let m = self.pairs.iter().sum::<f64>() / n;
        let var = self.pairs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0).max(1.0);
        format!("{:.1}% +- {:.1} ({} games, {} draws)", 100.0 * m, 100.0 * (var / n).sqrt(), 2 * self.pairs.len(), self.draws)
    }
}

/// `a` against `b` on `n` deck pairs, each played with both seatings: the
/// score of `a` overall; where a deck has a planeswalker, where none has;
/// where the decks have at most 7, 8-11, 12 or more nonland cards with an
/// activated ability (planeswalkers included).
fn match_up(a: &str, b: &str, n: u64, first_seed: u64) -> [Tally; 6] {
    let sp = spec();
    let mut t: [Tally; 6] = Default::default();
    let mut seen = BTreeMap::new();
    for s in 0..n {
        let seed = first_seed + s;
        let x = play(&sp, seed, [a, b], false, &mut seen);
        let y = play(&sp, seed, [b, a], false, &mut seen);
        let (pw, k) = deck_traits(&sp, seed);
        t[0].add(&x, 0, &y, 1);
        t[if pw { 1 } else { 2 }].add(&x, 0, &y, 1);
        t[match k {
            0..=7 => 3,
            8..=11 => 4,
            _ => 5,
        }]
        .add(&x, 0, &y, 1);
    }
    t
}

#[test]
#[ignore]
fn strength_against_the_old_policy_and_random() {
    let n = games(1000);
    for (a, b) in [("new", "old"), ("new", "random"), ("old", "random")] {
        let t = match_up(a, b, n, 1_000_000);
        println!("{a} vs {b}: {}", t[0].line());
        println!("  a deck with a planeswalker: {}", t[1].line());
        println!("  no planeswalker: {}", t[2].line());
        println!("  0-7 nonland cards with an activated ability: {}", t[3].line());
        println!("  8-11: {}", t[4].line());
        println!("  12 or more: {}", t[5].line());
    }
}

/// Games per second of bot-against-bot play (one thread), with the turns and
/// the decisions of two or more options per game, and the bots' share of the
/// time (`--test-threads=1` for clean numbers).
#[test]
#[ignore]
fn speed_of_bot_games() {
    let sp = spec();
    let n = games(1000);
    for kinds in [["old", "old"], ["new", "new"], ["new", "old"], ["old", "random"], ["new", "random"]] {
        let t0 = std::time::Instant::now();
        let (mut turns, mut decisions, mut in_bots) = (0u64, 0u64, 0f64);
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        for s in 0..n {
            let seed = 2_000_000 + s;
            let mut g = MtgAz::new_game(&sp, 2, seed).unwrap();
            let mut bots = [Bot::new(kinds[0], seed * 2 + 1), Bot::new(kinds[1], seed * 2 + 2)];
            while let Some(p) = g.to_move() {
                g.legal_actions(&mut legal);
                let a = if legal.len() == 1 {
                    legal[0]
                } else {
                    decisions += 1;
                    let t = std::time::Instant::now();
                    let a = bots[p as usize].choose(&g.g, &legal);
                    in_bots += t.elapsed().as_secs_f64();
                    a
                };
                keys.clear();
                g.step(a, &mut keys).unwrap();
            }
            turns += g.rounds() as u64;
        }
        let dt = t0.elapsed().as_secs_f64();
        println!(
            "{} vs {}: {:.0} games/s, {:.1} turns/game, {:.1} decisions/game, {:.2} us per bot decision, bots {:.0}% of the time ({n} games, {:.1} s)",
            kinds[0],
            kinds[1],
            n as f64 / dt,
            turns as f64 / n as f64,
            decisions as f64 / n as f64,
            1e6 * in_bots / decisions as f64,
            100.0 * in_bots / dt,
            dt
        );
    }
}

/// How often the new bot activates what, in new-vs-old games (a sanity
/// readout of the policy, not a test of strength).
#[test]
#[ignore]
fn activation_counts() {
    let sp = spec();
    let n = games(300);
    let mut by_name: BTreeMap<String, u64> = BTreeMap::new();
    let mut loyalty = [0u64; 3];
    for s in 0..n {
        let mut g = MtgAz::new_game(&sp, 2, 3_000_000 + s).unwrap();
        let mut bots = [Bot::new("new", s), Bot::new("old", s + 1)];
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal);
            let a = if legal.len() == 1 { legal[0] } else { bots[p as usize].choose(&g.g, &legal) };
            if p == 0 {
                let d = g.g.pending_decision().unwrap();
                if let (DecisionKind::Priority, Some(Choice::Ability(c, slot))) = (d.kind, d.choice_of(a)) {
                    let ad = g.g.def_of(c).activated_def(slot);
                    *by_name.entry(format!("{} {slot:?}", g.g.name(c))).or_default() += 1;
                    if let Some(l) = ad.loyalty {
                        loyalty[(l.signum() + 1) as usize] += 1;
                    }
                }
            }
            keys.clear();
            g.step(a, &mut keys).unwrap();
        }
    }
    let mut v: Vec<_> = by_name.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("loyalty activations (minus, zero, plus): {loyalty:?} in {n} games");
    for (k, c) in v {
        println!("{c:6} {k}");
    }
}

/// The new bot's choices other than passing in a few games against the old
/// policy (`BOTS_TRACE_SEED=s` picks the first game).
#[test]
#[ignore]
fn trace() {
    let sp = spec();
    let first = std::env::var("BOTS_TRACE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(5_000_000u64);
    for seed in first..first + games(3) {
        let [d0, d1] = sp.decks(seed);
        let gs = engine::GameSpec { max_turns: 60, log: true, ..engine::GameSpec::default() };
        let mut g = Game::new(CardDb::cube(), gs, [&d0, &d1], seed).unwrap();
        let mut bots = [Bot::new("new", seed), Bot::new("old", seed + 1)];
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        println!("== game {seed}");
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal);
            let a = if legal.len() == 1 { legal[0] } else { bots[p as usize].choose(&g, &legal) };
            if p == 0 && legal.len() > 1 && a != engine::act::PASS {
                println!("T{} {:?} {}", g.turn().number, g.turn().step, g.describe_action(a).unwrap());
            }
            if p == 0 && std::env::var("BOTS_TRACE_ALL").is_ok() && g.pending_decision().unwrap().kind == DecisionKind::Priority {
                println!("   options {:?}", g.options_desc());
            }
            keys.clear();
            g.apply(a, &mut keys).unwrap();
        }
        if let Ok(pat) = std::env::var("BOTS_TRACE_LOG") {
            for l in g.log_lines().iter().filter(|l| l.contains(&pat)) {
                println!("   log {l}");
            }
        }
        println!("result {:?} life {} / {}", g.result_values(), g.life(0), g.life(1));
    }
}

// ---- Edge of Eternities: the yardsticks' numbers (ENGINE.md 103) --------------

/// Where a measured game's decks come from (`BOTS_DECKS`, ENGINE.md 103.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    /// The cube's random decks of the runs' default spec (`MtgSpec::default()`).
    Cube,
    /// `FUZZ_POOL=eoe`'s decks (`tests/fuzz.rs::eoe_deck`): 17 random lands and 23 random spells
    /// of the EoE table's supported cards, every colour.
    EoeFuzz,
    /// The cube's two-colour generator (`az::decks::generate`) on the EoE table: two colours,
    /// 16-17 lands, singleton spells weighted to creatures and interaction (a stand-in for the
    /// S2 sealed builder until it lands).
    Eoe2c,
}

impl Source {
    fn parse(s: &str) -> Source {
        match s {
            "cube" => Source::Cube,
            "eoe" => Source::EoeFuzz,
            "eoe2c" => Source::Eoe2c,
            _ => panic!("BOTS_DECKS: {s:?} (cube, eoe or eoe2c)"),
        }
    }

    fn db(self) -> &'static CardDb {
        match self {
            Source::Cube => CardDb::cube(),
            Source::EoeFuzz | Source::Eoe2c => CardDb::eoe(),
        }
    }

    fn decks(self, seed: u64) -> [Vec<u16>; 2] {
        match self {
            Source::Cube => spec().decks(seed),
            Source::EoeFuzz => {
                // (The fuzz's own seeding: its games and these have the same decks.)
                let mut rng = Rng::new(seed ^ 0xDEC5);
                [eoe_fuzz_deck(&mut rng), eoe_fuzz_deck(&mut rng)]
            }
            Source::Eoe2c => {
                let mut rng = Rng::new(seed ^ 0xE0E2C);
                [eoe_2c_deck(&mut rng), eoe_2c_deck(&mut rng)]
            }
        }
    }
}

/// `BOTS_DECKS` (a comma-separated list; default `dflt`).
fn sources(dflt: &str) -> Vec<Source> {
    std::env::var("BOTS_DECKS").unwrap_or_else(|_| dflt.to_string()).split(',').map(Source::parse).collect()
}

/// The supported cards of the EoE table: its lands (the basics among them) and its other cards
/// (as `tests/fuzz.rs::eoe_pool`).
fn eoe_pool() -> &'static (Vec<u16>, Vec<u16>) {
    static POOL: std::sync::OnceLock<(Vec<u16>, Vec<u16>)> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let db = CardDb::eoe();
        let (mut lands, mut spells) = (Vec::new(), Vec::new());
        for name in db.supported() {
            let id = db.id(name).unwrap();
            if db.def(id).is(types::LAND) {
                lands.push(id);
            } else {
                spells.push(id);
            }
        }
        (lands, spells)
    })
}

/// `FUZZ_POOL=eoe`'s deck: 17 lands, 23 spells, each drawn uniformly with replacement.
fn eoe_fuzz_deck(rng: &mut Rng) -> Vec<u16> {
    let (lands, spells) = eoe_pool();
    let mut d: Vec<u16> = (0..17).map(|_| lands[rng.below(lands.len() as u32) as usize]).collect();
    d.extend((0..23).map(|_| spells[rng.below(spells.len() as u32) as usize]));
    d
}

/// A two-colour EoE deck of supported cards (`az::decks::generate` on the EoE table, drawn
/// again while it holds an unsupported card).
fn eoe_2c_deck(rng: &mut Rng) -> Vec<u16> {
    let db = CardDb::eoe();
    loop {
        let d = engine::az::decks::generate(db, rng, &engine::az::decks::DeckGen::default());
        if d.iter().all(|&id| db.id(&db.def(id).name) == Some(id)) {
            return d;
        }
    }
}

/// One measured game's numbers.
#[derive(Clone, Default)]
struct Measured {
    result: [f32; 2],
    /// The turn cap (a draw).
    capped: bool,
    aborted: bool,
    turns: u32,
    /// Decisions of two or more options, per seat, and the time the seat's bot took on them.
    decisions: [u64; 2],
    secs: [f64; 2],
    /// How it ended: "life", "library", "poison", "effect", "turns", "aborted", "both".
    end: &'static str,
}

/// A game of `src`'s decks for `seed`, `kinds[s]` in seat `s`, played to the end. An engine
/// panic is an engine bug (reported, ENGINE.md 103): the game is named and ends "panic", left
/// out of the scores.
fn measure(src: Source, seed: u64, kinds: [&str; 2]) -> Measured {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| measure_game(src, seed, kinds))).unwrap_or_else(|_| {
        eprintln!("ENGINE PANIC: {src:?} seed {seed} {kinds:?}");
        Measured { end: "panic", ..Measured::default() }
    })
}

fn measure_game(src: Source, seed: u64, kinds: [&str; 2]) -> Measured {
    let [d0, d1] = src.decks(seed);
    let gs = engine::GameSpec { max_turns: 60, ..engine::GameSpec::default() };
    let mut g = Game::new(src.db(), gs, [&d0, &d1], seed).unwrap();
    let mut bots = [Bot::new(kinds[0], seed * 2 + 1), Bot::new(kinds[1], seed * 2 + 2)];
    let mut m = Measured::default();
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    let mut steps = 0u32;
    while let Some(p) = g.to_move() {
        g.legal_actions(&mut legal);
        let a = if legal.len() == 1 {
            legal[0]
        } else {
            let t = std::time::Instant::now();
            let a = bots[p as usize].choose(&g, &legal);
            m.secs[p as usize] += t.elapsed().as_secs_f64();
            m.decisions[p as usize] += 1;
            assert!(legal.contains(&a), "{src:?} seed {seed}: {} answered {a} ({legal:?})", kinds[p as usize]);
            a
        };
        keys.clear();
        g.apply(a, &mut keys).unwrap();
        steps += 1;
        assert!(steps < 500_000, "{src:?} seed {seed} ({kinds:?}): the game does not end");
    }
    let o = g.outcome().unwrap();
    m.result = g.result_values();
    m.turns = g.turn().number;
    m.capped = o.reason == engine::game::EndReason::TurnLimit;
    m.aborted = o.reason == engine::game::EndReason::Aborted;
    m.end = match (o.reason, o.winner) {
        (engine::game::EndReason::TurnLimit, _) => "turns",
        (engine::game::EndReason::Aborted, _) => "aborted",
        (engine::game::EndReason::Effect, _) => "effect",
        (_, None) => "both",
        (_, Some(w)) => {
            let l = g.player(1 - w);
            if g.life(1 - w) <= 0 {
                "life"
            } else if l.poison >= 10 {
                "poison"
            } else if l.drew_from_empty || l.library.is_empty() {
                "library"
            } else {
                "other"
            }
        }
    };
    m
}

/// `f(seed)` for `n` seeds from `first`, on `BOTS_THREADS` threads (default 8), in seed order.
fn par_map<T: Send + Clone + Default>(first: u64, n: u64, f: impl Fn(u64) -> T + Sync) -> Vec<T> {
    let threads = std::env::var("BOTS_THREADS").ok().and_then(|s| s.parse().ok()).unwrap_or(8u64).max(1);
    let mut out = vec![T::default(); n as usize];
    let chunk = n.div_ceil(threads).max(1) as usize;
    std::thread::scope(|s| {
        for (k, part) in out.chunks_mut(chunk).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (j, x) in part.iter_mut().enumerate() {
                    *x = f(first + (k * chunk + j) as u64);
                }
            });
        }
    });
    out
}

/// `a` against `b` on `n` deck pairs of `src`, each played with both seatings: one line of
/// `a`'s score (its standard error over deck pairs and the 95% interval), the draws, how the
/// games ended, their turns and decisions, and each bot kind's time per decision.
fn yardstick_line(src: Source, a: &str, b: &str, n: u64, first_seed: u64) -> String {
    let mut games = par_map(first_seed, n, |seed| (measure(src, seed, [a, b]), measure(src, seed, [b, a])));
    let panics = games.iter().filter(|(x, y)| x.end == "panic" || y.end == "panic").count();
    games.retain(|(x, y)| x.end != "panic" && y.end != "panic");
    let pairs: Vec<f64> = games.iter().map(|(x, y)| (x.result[0] + y.result[1]) as f64 / 2.0).collect();
    let k = pairs.len() as f64;
    let mean = pairs.iter().sum::<f64>() / k;
    let se = (pairs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt();
    let all: Vec<&Measured> = games.iter().flat_map(|(x, y)| [x, y]).collect();
    let g = all.len() as f64;
    let mut ends: BTreeMap<&str, u64> = BTreeMap::new();
    for m in &all {
        *ends.entry(m.end).or_default() += 1;
    }
    let capped = all.iter().filter(|m| m.capped).count();
    let turns = all.iter().map(|m| m.turns as f64).sum::<f64>() / g;
    let dec = all.iter().map(|m| (m.decisions[0] + m.decisions[1]) as f64).sum::<f64>() / g;
    // Per kind: decisions and seconds (seat 0 of the first game and seat 1 of the second are `a`).
    let (mut da, mut sa, mut db_, mut sb) = (0u64, 0f64, 0u64, 0f64);
    for (x, y) in &games {
        da += x.decisions[0] + y.decisions[1];
        sa += x.secs[0] + y.secs[1];
        db_ += x.decisions[1] + y.decisions[0];
        sb += x.secs[1] + y.secs[0];
    }
    format!(
        "{src:?} {a} vs {b}: {:.1}% +- {:.1} (95% {:.1}..{:.1}), {} games, {} turn-limit draws ({:.1}%), ends {ends:?}, {:.1} turns, {:.0} decisions/game ({a} {:.0}, {b} {:.0}), {a} {:.1} us/decision, {b} {:.1} us/decision, {panics} deck pairs left out (engine panics)",
        100.0 * mean,
        100.0 * se,
        100.0 * (mean - 1.96 * se),
        100.0 * (mean + 1.96 * se),
        all.len(),
        capped,
        100.0 * capped as f64 / g,
        turns,
        dec,
        da as f64 / g,
        db_ as f64 / g,
        1e6 * sa / da.max(1) as f64,
        1e6 * sb / db_.max(1) as f64,
    )
}

/// The yardsticks on each deck source (`BOTS_DECKS`, default `eoe,eoe2c,cube`): the heuristic
/// against random and against itself, random against random (`BOTS_GAMES` deck pairs, default
/// 1000, each played both ways; `BOTS_MATCHUPS` e.g. `new-random,new-new`).
#[test]
#[ignore]
fn eoe_yardsticks() {
    let n = games(1000);
    let matchups = std::env::var("BOTS_MATCHUPS").unwrap_or_else(|_| "new-random,new-new,random-random".into());
    for src in sources("eoe,eoe2c,cube") {
        for mu in matchups.split(',') {
            let (a, b) = mu.split_once('-').expect("BOTS_MATCHUPS: a-b,...");
            let t0 = std::time::Instant::now();
            let line = yardstick_line(src, a, b, n, 6_000_000);
            println!("{line} [{:.1} s]", t0.elapsed().as_secs_f64());
        }
    }
}

/// The EoE table's supported cards by the set's mechanics the heuristic reads (warp, void,
/// station, Landers): name, mana value, warp cost, triggers (event), and which of the words its
/// definition holds (a readout from `Debug`, not a test).
#[test]
#[ignore]
fn eoe_inventory() {
    let db = CardDb::eoe();
    let (lands, spells) = eoe_pool();
    for &id in lands.iter().chain(spells) {
        let d = db.def(id);
        let s = format!("{d:?}");
        let warp: Vec<String> = d.alt_costs.iter().filter(|a| a.warp).map(|a| format!("warp mv {}", a.mana.mana_value())).collect();
        let trig: Vec<String> = d.triggers.iter().map(|t| format!("{:?}", t.event)).collect();
        let words: Vec<&str> = ["Void", "Charge", "Lander", "TappedPower", "Search"].into_iter().filter(|w| s.contains(w)).collect();
        println!("{} | mv {} | types {:#x} | {} | trig {} | {}", d.name, d.mana_value(), d.types, warp.join(","), trig.join(","), words.join(","));
    }
}

/// What the heuristic does with what it is offered, in heuristic-vs-heuristic games on
/// `BOTS_DECKS` (default eoe2c; `BOTS_GAMES` games, default 300): per activated ability (by card
/// and slot), the turns it was offered at priority and the times it was activated; the
/// alternative costs offered and chosen (`ChooseAltCost`); the casts offered from zones other
/// than the hand and taken.
#[test]
#[ignore]
fn eoe_mechanics() {
    let src = sources("eoe2c")[0];
    let n = games(300);
    let opp = std::env::var("BOTS_OPP").unwrap_or_else(|_| "new".into());
    // name -> (turns offered, activated)
    let mut acts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut alts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut zones: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for s in 0..n {
        let seed = 7_000_000 + s;
        let [d0, d1] = src.decks(seed);
        let gs = engine::GameSpec { max_turns: 60, ..engine::GameSpec::default() };
        let mut g = Game::new(src.db(), gs, [&d0, &d1], seed).unwrap();
        let mut bots = [Bot::new("new", seed * 2 + 1), Bot::new(&opp, seed * 2 + 2)];
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        let mut seen: std::collections::BTreeSet<(u32, CardIdx, String)> = Default::default();
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal);
            let a = if legal.len() == 1 { legal[0] } else { bots[p as usize].choose(&g, &legal) };
            if p == 0 && legal.len() > 1 {
                let d = g.pending_decision().unwrap();
                let t = g.turn().number;
                match d.kind {
                    DecisionKind::Priority => {
                        for o in &d.options {
                            let key = match o.choice {
                                Choice::Ability(c, slot) => format!("{} {slot:?}", g.name(c)),
                                Choice::Cast(c) if g.obj(c).zone != Zone::Hand => format!("cast {} from {:?}", g.name(c), g.obj(c).zone),
                                _ => continue,
                            };
                            let (c, map) = match o.choice {
                                Choice::Ability(c, _) => (c, &mut acts),
                                Choice::Cast(c) => (c, &mut zones),
                                _ => unreachable!(),
                            };
                            let e = map.entry(key.clone()).or_default();
                            if seen.insert((t, c, key)) {
                                e.0 += 1;
                            }
                            if o.action == a {
                                e.1 += 1;
                            }
                        }
                    }
                    DecisionKind::ChooseAltCost => {
                        let card = g.stack().last().map(|s| s.kind);
                        let name = match card {
                            Some(engine::game::StackKind::Spell { card }) => g.name(card).to_string(),
                            _ => "?".into(),
                        };
                        for o in &d.options {
                            let e = alts.entry(format!("{name}: {:?}", o.choice)).or_default();
                            e.0 += 1;
                            if o.action == a {
                                e.1 += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            keys.clear();
            g.apply(a, &mut keys).unwrap();
        }
    }
    for (title, map) in [("activated abilities", &acts), ("alternative costs", &alts), ("casts from other zones", &zones)] {
        let mut v: Vec<_> = map.iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1 .0));
        println!("== {title} (offered, taken) in {n} games ({src:?}, seat 0)");
        for (k, (o, t)) in v {
            println!("{o:6} {t:6} {k}");
        }
    }
}

/// A logged game of the heuristic (`BOTS_HEUR`, default new; seat `BOTS_SEAT`, default 0) against
/// `BOTS_OPP` (default random) on `BOTS_DECKS` (default eoe), the bots seeded as in
/// `eoe_yardsticks`: its decks, then each decision of the heuristic, or the game log
/// (`BOTS_TRACE_LOG=1`), for `BOTS_TRACE_SEED` and the next `BOTS_GAMES - 1` seeds. An engine
/// panic prints the log so far and the action taken.
#[test]
#[ignore]
fn eoe_trace() {
    let src = sources("eoe")[0];
    let first = std::env::var("BOTS_TRACE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(6_000_000u64);
    let seat: u8 = std::env::var("BOTS_SEAT").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let opp = std::env::var("BOTS_OPP").unwrap_or_else(|_| "random".into());
    let heur = std::env::var("BOTS_HEUR").unwrap_or_else(|_| "new".into());
    for seed in first..first + games(1) {
        let [d0, d1] = src.decks(seed);
        let db = src.db();
        for (p, d) in [&d0, &d1].into_iter().enumerate() {
            let mut names: Vec<&str> = d.iter().map(|&x| db.def(x).name.as_str()).collect();
            names.sort_unstable();
            println!("deck {p}: {}", names.join(", "));
        }
        let gs = engine::GameSpec { max_turns: 60, log: true, ..engine::GameSpec::default() };
        let mut g = Game::new(db, gs, [&d0, &d1], seed).unwrap();
        let mut kinds = [opp.as_str(), opp.as_str()];
        kinds[seat as usize] = heur.as_str();
        let mut bots = [Bot::new(kinds[0], seed * 2 + 1), Bot::new(kinds[1], seed * 2 + 2)];
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        let full = std::env::var("BOTS_TRACE_LOG").is_ok();
        println!("== game {seed} ({src:?}), the heuristic in seat {seat}");
        while let Some(p) = g.to_move() {
            g.legal_actions(&mut legal);
            let a = if legal.len() == 1 { legal[0] } else { bots[p as usize].choose(&g, &legal) };
            if !full && p == seat && legal.len() > 1 {
                let d = g.pending_decision().unwrap();
                println!("T{} {:?} {:?}: {} (of {})", g.turn().number, g.turn().step, d.kind, g.describe_action(a).unwrap(), legal.len());
            }
            keys.clear();
            let what = g.describe_action(a).unwrap_or_default();
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.apply(a, &mut keys).unwrap())).is_err() {
                println!("ENGINE PANIC at T{} {:?} applying {what}", g.turn().number, g.turn().step);
                for l in g.log_lines() {
                    println!("   {l}");
                }
                break;
            }
        }
        if full {
            for l in g.log_lines() {
                println!("   {l}");
            }
        }
        println!("result {:?} {:?} life {} / {}", g.result_values(), g.outcome().map(|o| o.reason), g.life(0), g.life(1));
    }
}

// ---- the policy in set situations --------------------------------------------

fn bot_choice(g: &Game) -> Choice {
    let d = g.pending_decision().expect("a decision");
    let legal: Vec<u16> = d.options.iter().map(|o| o.action).collect();
    let a = Heuristic::new(1).choose(g, &legal);
    d.choice_of(a).unwrap()
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn ability(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, engine::db::AbilitySlot::Activated(i))
}

fn lands(n: usize, name: &'static str) -> Vec<&'static str> {
    vec![name; n]
}

#[test]
fn removes_a_creature_with_a_minus_else_uses_the_plus() {
    // Chandra, Torch of Defiance (loyalty 4): -3 deals 4 damage to a creature.
    let mut b = lands(4, "Mountain");
    b.push("Chandra, Torch of Defiance");
    let g = Scenario::new(db()).battlefield(0, &b).battlefield(1, &["Test Trampler"]).library(0, &["Mountain"; 5]).start();
    assert_eq!(bot_choice(&g), ability(bf(&g, 0, "Chandra, Torch of Defiance"), 2), "{:?}", g.options_desc());
    // Nothing to remove: the +1 that exiles and casts, not the mana one.
    let g = Scenario::new(db()).battlefield(0, &b).library(0, &["Mountain"; 5]).start();
    assert_eq!(bot_choice(&g), ability(bf(&g, 0, "Chandra, Torch of Defiance"), 0), "{:?}", g.options_desc());
}

#[test]
fn takes_the_ultimate_when_it_can() {
    let g = Scenario::new(db())
        .battlefield(0, &["Chandra, Torch of Defiance"])
        .counters(0, "Chandra, Torch of Defiance", Counter::Loyalty, 8)
        .library(0, &["Mountain"; 5])
        .start();
    assert_eq!(g.counters_on(bf(&g, 0, "Chandra, Torch of Defiance"), Counter::Loyalty), 8);
    assert_eq!(bot_choice(&g), ability(bf(&g, 0, "Chandra, Torch of Defiance"), 3));
}

#[test]
fn a_minus_aimed_at_the_opponent_never_hits_its_own_permanents() {
    // Teferi, Hero of Dominaria's -3 puts a nonland permanent into its
    // owner's library: with only its own permanents to hit, the +1.
    let g = Scenario::new(db())
        .battlefield(0, &["Teferi, Hero of Dominaria", "Test Trampler"])
        .battlefield(1, &["Island"])
        .library(0, &["Island"; 5])
        .start();
    assert_eq!(bot_choice(&g), ability(bf(&g, 0, "Teferi, Hero of Dominaria"), 0));
}

#[test]
fn cracks_a_fetch_land_first() {
    let g = Scenario::new(db())
        .battlefield(0, &["Polluted Delta"])
        .hand(0, &["Test Bear"])
        .library(0, &["Island", "Swamp", "Island"])
        .lands_played(0, 1)
        .start();
    assert_eq!(bot_choice(&g), ability(bf(&g, 0, "Polluted Delta"), 0));
}

#[test]
fn equips_once_and_moves_on() {
    let mut g = Scenario::new(db())
        .battlefield(0, &["Lightning Greaves", "Test Bear", "Test Trampler"])
        .library(0, &["Island"; 5])
        .start();
    let greaves = bf(&g, 0, "Lightning Greaves");
    let mut h = Heuristic::new(3);
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    let mut equips = 0;
    while g.turn().step == Step::Main1 {
        g.legal_actions(&mut legal);
        let a = h.choose(&g, &legal);
        let d = g.pending_decision().unwrap();
        if d.kind == DecisionKind::Priority && d.choice_of(a) == Some(ability(greaves, 0)) {
            equips += 1;
        }
        if d.kind == DecisionKind::Target {
            // The best creature that can attack.
            assert_eq!(d.choice_of(a), Some(g.tgt(bf(&g, 0, "Test Trampler"))));
        }
        g.apply(a, &mut keys).unwrap();
        assert!(equips <= 1, "equipped twice in one main phase");
    }
    assert_eq!(equips, 1);
    assert_eq!(g.obj(greaves).attached.map(|r| r.idx), Some(bf(&g, 0, "Test Trampler")));
}

#[test]
fn crews_with_a_creature_that_would_not_attack() {
    // The Bear would die attacking into the Giant; the Copter flies over it.
    let mut g = Scenario::new(db())
        .battlefield(0, &["Smuggler's Copter", "Test Bear"])
        .battlefield(1, &["Test Giant"])
        .library(0, &["Island"; 5])
        .start();
    let copter = bf(&g, 0, "Smuggler's Copter");
    assert_eq!(bot_choice(&g), ability(copter, 0), "{:?}", g.options_desc());
    // And it attacks with the Copter.
    let mut h = Heuristic::new(3);
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    for _ in 0..100 {
        if g.obj(copter).attacking || g.turn().step == Step::Main2 {
            break;
        }
        g.legal_actions(&mut legal);
        let a = h.choose(&g, &legal);
        g.apply(a, &mut keys).unwrap();
    }
    assert!(g.obj(copter).attacking);
    assert!(!g.obj(bf(&g, 0, "Test Bear")).attacking);
}

#[test]
fn free_and_untap_abilities_do_not_loop() {
    // Basalt Monolith can untap itself with its own mana; Sensei's Divining
    // Top's look costs {1}: neither is worth anything to this bot, and the
    // main phase ends.
    let mut g = Scenario::new(db())
        .battlefield(0, &["Basalt Monolith", "Sensei's Divining Top", "Island", "Island", "Island"])
        .library(0, &["Island"; 10])
        .start();
    let mut h = Heuristic::new(3);
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    let mut n = 0;
    while g.turn().step == Step::Main1 {
        g.legal_actions(&mut legal);
        let a = h.choose(&g, &legal);
        g.apply(a, &mut keys).unwrap();
        n += 1;
        assert!(n < 50, "{:?}", g.log_lines());
    }
}

#[test]
fn draws_with_spare_mana_at_the_end_of_the_opponents_turn() {
    // Mind Stone's draw sacrifices a mana source: only when flooded. A Clue
    // is cracked at the opponent's end step, not in its own main phase.
    let mut b = lands(3, "Island");
    b.push("Mind Stone");
    let g = Scenario::new(db()).battlefield(0, &b).library(0, &["Island"; 5]).library(1, &["Island"; 5]).at(Step::End, 1).start();
    let stone = bf(&g, 0, "Mind Stone");
    if g.pending_decision().is_some_and(|d| d.player == 0 && d.kind == DecisionKind::Priority) {
        assert_ne!(bot_choice(&g), ability(stone, 1));
    }
    let mut b = lands(6, "Island");
    b.push("Mind Stone");
    let mut g = Scenario::new(db()).battlefield(0, &b).library(0, &["Island"; 5]).library(1, &["Island"; 5]).at(Step::End, 1).start();
    let stone = bf(&g, 0, "Mind Stone");
    g.pass_until(|g| g.pending_decision().is_some_and(|d| d.player == 0));
    assert_eq!(bot_choice(&g), ability(stone, 1), "{:?}", g.options_desc());
}

#[test]
fn casts_x_spells_for_as_much_as_it_can_pay() {
    let mut g = Scenario::new(db())
        .battlefield(0, &["Island", "Island", "Island", "Island"])
        .hand(0, &["Walking Ballista"])
        .library(0, &["Island"; 5])
        .lands_played(0, 1)
        .start();
    let mut h = Heuristic::new(3);
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    while g.turn().step == Step::Main1 {
        g.legal_actions(&mut legal);
        let a = h.choose(&g, &legal);
        g.apply(a, &mut keys).unwrap();
    }
    let ballista = bf(&g, 0, "Walking Ballista");
    assert_eq!(g.counters_on(ballista, Counter::P1P1), 2);
}

// ---- Edge of Eternities' mechanics (ENGINE.md 103) ---------------------------

fn eoe_choice(g: &Game, h: Heuristic) -> Choice {
    let mut h = h;
    let d = g.pending_decision().expect("a decision");
    let legal: Vec<u16> = d.options.iter().map(|o| o.action).collect();
    let a = h.choose(g, &legal);
    d.choice_of(a).unwrap()
}

/// The station ability's slot of a station card (CR 702.184a: the one whose
/// cost taps another creature).
fn station_slot(g: &Game, c: CardIdx) -> Choice {
    let i = g.def_of(c).activated.iter().position(|a| a.cost.tap_untapped.is_some()).expect("a station ability");
    ability(c, i as u8)
}

/// Drive `h` (both seats) until the step leaves `step`.
fn drive_step(g: &mut Game, h: &mut Heuristic, step: Step) {
    let mut legal = Vec::new();
    let mut keys = Vec::new();
    for _ in 0..200 {
        if g.turn().step != step || g.is_over() {
            return;
        }
        g.legal_actions(&mut legal);
        let a = h.choose(g, &legal);
        g.apply(a, &mut keys).unwrap();
    }
    panic!("{step:?} did not end");
}

#[test]
fn a_land_search_that_costs_mana_waits_for_spare_mana() {
    // Kav Landseeker's Lander ({2}, {T}, sacrifice: a basic land onto the
    // battlefield tapped) with two mana left and a two-mana spell in hand:
    // the spell (the M7c policy cracked the Lander first, before spells).
    let start = || {
        Scenario::new(CardDb::eoe())
            .battlefield(0, &["Mountain"; 6])
            .hand(0, &["Kav Landseeker", "Wurmwall Sweeper"])
            .library(0, &["Mountain"; 8])
            .library(1, &["Plains"; 8])
            .lands_played(0, 1)
            .start()
    };
    let mut g = start();
    drive_step(&mut g, &mut Heuristic::new(3), Step::Main1);
    assert!(g.find_card(0, "Wurmwall Sweeper", Zone::Battlefield).is_some(), "the spell was cast");
    assert!(g.permanents_of(0).any(|c| g.name(c) == "Lander"), "the Lander kept for spare mana");
    let mut g = start();
    drive_step(&mut g, &mut Heuristic::m7c(3), Step::Main1);
    assert!(g.find_card(0, "Wurmwall Sweeper", Zone::Hand).is_some(), "M7c: the Lander first, no mana left for the spell");
}

#[test]
fn stations_with_a_creature_that_would_neither_attack_nor_block_up_to_the_highest_threshold() {
    // Warmaker Gunship: station 6+ (a 4/3 flyer). After combat, Kav
    // Landseeker (4 power) taps for it.
    let start = |charge: u64, theirs: &[&str]| {
        let mut s = Scenario::new(CardDb::eoe())
            .battlefield(0, &["Warmaker Gunship", "Kav Landseeker"])
            .battlefield(1, theirs)
            .library(0, &["Mountain"; 5])
            .library(1, &["Plains"; 5])
            .at(Step::Main2, 0);
        if charge > 0 {
            let k = Counter::Named(CardDb::eoe().subtype_id("counter:charge").expect("the charge counter"));
            s = s.counters(0, "Warmaker Gunship", k, charge as _);
        }
        s.start()
    };
    let g = start(0, &[]);
    let gunship = bf(&g, 0, "Warmaker Gunship");
    assert_eq!(eoe_choice(&g, Heuristic::new(1)), station_slot(&g, gunship), "{:?}", g.options_desc());
    // At its highest threshold, more counters do nothing.
    let g = start(6, &[]);
    assert_ne!(eoe_choice(&g, Heuristic::new(1)), station_slot(&g, bf(&g, 0, "Warmaker Gunship")));
    // (The M7c policy stationed anyway.)
    assert_eq!(eoe_choice(&g, Heuristic::m7c(1)), station_slot(&g, bf(&g, 0, "Warmaker Gunship")));
    // Kav Landseeker would block Haliya and kill it: kept back.
    let g = start(0, &["Haliya, Guided by Light"]);
    assert_ne!(eoe_choice(&g, Heuristic::new(1)), station_slot(&g, bf(&g, 0, "Warmaker Gunship")));
}

#[test]
fn a_cast_only_warp_pays_needs_something_the_card_does_in_its_stay() {
    // Exalted Sunborn (warp {1}{W}) does nothing as it enters: with two
    // Plains, not warped (the M7c policy warped it, to be exiled at the end
    // step). Sinister Cryologist (warp {U}) has an enters trigger: warped.
    let g = Scenario::new(CardDb::eoe())
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Exalted Sunborn"])
        .library(0, &["Plains"; 5])
        .library(1, &["Plains"; 5])
        .lands_played(0, 1)
        .start();
    let sunborn = g.card(0, "Exalted Sunborn", Zone::Hand);
    assert_eq!(eoe_choice(&g, Heuristic::new(1)), Choice::Pass, "{:?}", g.options_desc());
    assert_eq!(eoe_choice(&g, Heuristic::m7c(1)), Choice::Cast(sunborn));
    let g = Scenario::new(CardDb::eoe())
        .battlefield(0, &["Island"])
        .battlefield(1, &["Haliya, Guided by Light"])
        .hand(0, &["Sinister Cryologist"])
        .library(0, &["Island"; 5])
        .library(1, &["Plains"; 5])
        .lands_played(0, 1)
        .start();
    assert_eq!(eoe_choice(&g, Heuristic::new(1)), Choice::Cast(g.card(0, "Sinister Cryologist", Zone::Hand)));
}

#[test]
fn a_free_alternative_cost_before_the_mana_cost() {
    // Weftwalking: the first spell each player casts during each of their
    // turns may be cast without paying its mana cost (CR 118.9).
    let mut g = Scenario::new(CardDb::eoe())
        .battlefield(0, &["Weftwalking", "Island", "Island", "Island"])
        .hand(0, &["Sinister Cryologist"])
        .library(0, &["Island"; 5])
        .library(1, &["Plains"; 5])
        .lands_played(0, 1)
        .spec(|s| s.auto_single = false)
        .start();
    let cryo = g.card(0, "Sinister Cryologist", Zone::Hand);
    assert_eq!(eoe_choice(&g, Heuristic::new(1)), Choice::Cast(cryo));
    g.choose(Choice::Cast(cryo));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseAltCost), "{:?}", g.options_desc());
    let free = Choice::Number((engine::act::NUMBER_N - 1) as u8);
    assert!(g.has_option(free), "{:?}", g.options_desc());
    assert_eq!(eoe_choice(&g, Heuristic::new(1)), free);
    assert_eq!(eoe_choice(&g, Heuristic::m7c(1)), Choice::Number(0), "M7c: the mana cost");
}

#[test]
fn a_spell_better_with_void_waits_for_the_second_main_phase() {
    // Plasma Bolt: more damage with void. No void yet in the first main
    // phase: kept for the second (combat may make void).
    let start = |step: Step| {
        Scenario::new(CardDb::eoe())
            .battlefield(0, &["Mountain"])
            .battlefield(1, &["Haliya, Guided by Light"])
            .hand(0, &["Plasma Bolt"])
            .library(0, &["Mountain"; 5])
            .library(1, &["Plains"; 5])
            .lands_played(0, 1)
            .at(step, 0)
            .start()
    };
    let g = start(Step::Main1);
    assert!(!g.void_active());
    let bolt = g.card(0, "Plasma Bolt", Zone::Hand);
    assert_eq!(eoe_choice(&g, Heuristic::new(1)), Choice::Pass, "{:?}", g.options_desc());
    assert_eq!(eoe_choice(&g, Heuristic::m7c(1)), Choice::Cast(bolt));
    let g = start(Step::Main2);
    assert_eq!(eoe_choice(&g, Heuristic::new(1)), Choice::Cast(g.card(0, "Plasma Bolt", Zone::Hand)));
}

// ---- the inventory ----------------------------------------------------------

fn op_names(ab: &engine::db::AbilityDef) -> String {
    let mut v: Vec<String> = Vec::new();
    let push = |v: &mut Vec<String>, ops: &[engine::program::Op]| {
        for op in ops {
            let s = format!("{op:?}");
            let cut = s.find([' ', '{', '(']).unwrap_or(s.len());
            let mut name = s[..cut].to_string();
            if name == "Ext" {
                let inner = &s[4..];
                let c2 = inner.find([' ', '{', '(', ')']).unwrap_or(inner.len());
                name = format!("Ext:{}", &inner[..c2]);
            }
            v.push(name);
        }
    };
    push(&mut v, &ab.program);
    if let Some(m) = &ab.modes {
        for (i, x) in m.list.iter().enumerate() {
            v.push(format!("|mode{i}:"));
            push(&mut v, &x.program);
        }
    }
    v.join(" ")
}

fn describe(ad: &ActivatedDef) -> String {
    let c = &ad.cost;
    let mut cost = Vec::new();
    if !c.mana.is_zero() {
        cost.push(format!("mana{}", c.mana.mana_value()));
    }
    for (on, s) in [
        (c.tap, "T"),
        (c.sac_self, "sacself"),
        (c.exile_self, "exileself"),
        (c.discard_self, "discardself"),
        (c.sacrifice.is_some(), "sac"),
        (c.remove_counters.is_some(), "remcounters"),
        (c.bounce.is_some(), "bounce"),
        (c.exile_graveyard > 0, "exilegy"),
        (ad.sorcery, "sorcery"),
    ] {
        if on {
            cost.push(s.to_string());
        }
    }
    if c.life > 0 {
        cost.push(format!("life{}", c.life));
    }
    if c.discard > 0 {
        cost.push(format!("discard{}", c.discard));
    }
    if c.crew > 0 {
        cost.push(format!("crew{}", c.crew));
    }
    if let Some(l) = ad.loyalty {
        cost.push(format!("loyalty{l:+}"));
    }
    let eff = match &ad.effect {
        ActEffect::Mana(_) => "MANA".to_string(),
        ActEffect::Stack(ab) => format!("targets{} {}", ab.targets.len(), op_names(ab)),
    };
    format!("[{}] {}", cost.join(","), eff)
}

/// The cube's activated abilities that are not mana abilities paid while
/// paying (the inventory the bot's rules are written against).
#[test]
#[ignore]
fn activated_inventory() {
    let db = CardDb::cube();
    for id in 0..db.len() as u16 {
        let d = db.def(id);
        let mut lines = Vec::new();
        for ad in &d.activated {
            if ad.mana().is_some() && !ad.instant_only {
                continue;
            }
            lines.push(format!("  act {}", describe(ad)));
        }
        for ad in &d.hand_abilities {
            lines.push(format!("  hand {}", describe(ad)));
        }
        for ad in &d.gy_abilities {
            lines.push(format!("  gy {}", describe(ad)));
        }
        if !lines.is_empty() {
            let kind = if d.token { " token" } else if db.is_face(id) { " face" } else { "" };
            println!("{} ({:#x}{kind})", d.name, d.types);
            for l in lines {
                println!("{l}");
            }
        }
    }
}
