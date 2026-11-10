//! The heuristic bot (ENGINE.md 21.4): it answers every decision kind with a
//! legal id and every game ends (a loop of optional actions ends when it
//! comes back to a state, `engine/src/loops.rs`), it uses loyalty and
//! other activated abilities in the situations its policy names, and its
//! strength against the policy before activated abilities (`HeuristicV1`)
//! and random. The long measurements are `#[ignore]`d:
//! `cargo test --release -p engine --test bots -- --ignored --nocapture`
//! (`BOTS_GAMES=n` sets the number of deck pairs).

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
