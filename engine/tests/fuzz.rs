//! Random-play fuzzer (ENGINE.md 15): complete games between random decks
//! of supported cards, checking after every action that nothing panics, the
//! structural invariants hold (every card in exactly one zone), legal
//! actions are sorted, unique and non-empty, and the game terminates; that
//! the same seed and actions replay bit for bit; that a clone continues
//! identically; that `determinize` keeps the decider's view; and that every
//! option of a sampled decision can be completed, in the game and in the
//! worlds `determinize` samples.
//!
//! `FUZZ_GAMES=n` sets the number of games (default 300).

mod common;

use engine::rng::Rng;
use engine::{Choice, DecisionKind, Game, GameSpec, Outcome, NUM_ACTIONS};
use std::sync::atomic::{AtomicU64, Ordering};

struct Played {
    actions: Vec<u16>,
    keys: Vec<u64>,
    fingerprint: u64,
    outcome: Outcome,
    turns: u32,
}

fn decks(seed: u64) -> (Vec<u16>, Vec<u16>) {
    let db = common::db();
    let mut rng = Rng::new(seed ^ 0xDEC5);
    (common::random_deck(db, &mut rng), common::random_deck(db, &mut rng))
}

fn new_game(seed: u64) -> Game {
    let (d0, d1) = decks(seed);
    // `FUZZ_LOG=1` keeps the game log, so a panic message shows the game.
    let log = std::env::var("FUZZ_LOG").is_ok();
    Game::new(common::db(), GameSpec { log, ..GameSpec::default() }, [&d0, &d1], seed).unwrap()
}

/// Play one game with random actions; run the extra checks along the way.
fn play(seed: u64, checks: bool) -> Played {
    let mut g = new_game(seed);
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 1);
    // (The completion check's sampling: its own generator, so the game's
    // actions are the same with or without it.)
    let mut crng = Rng::new(seed ^ 0xC0_4D_1E7E);
    let mut legal = Vec::new();
    let mut actions = Vec::new();
    let mut keys = Vec::new();
    let mut steps = 0;
    while !g.is_over() {
        g.legal_actions(&mut legal);
        assert!(!legal.is_empty(), "seed {seed}: no legal action in a running game");
        assert!(legal.windows(2).all(|w| w[0] < w[1]), "seed {seed}: actions not sorted/unique");
        assert!(legal.iter().all(|&a| (a as usize) < NUM_ACTIONS));
        let a = legal[rng.below(legal.len() as u32) as usize];
        if checks && rng.below(40) == 0 {
            check_clone(&g, a, seed);
            check_determinize(&g, seed, &mut rng);
        }
        if checks && complete_now(&g, &mut crng) {
            check_options_complete(&g, seed, "the game");
        }
        g.apply(a, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        g.check_invariants().unwrap_or_else(|e| panic!("seed {seed} step {steps}: {e}
{}", g.log_lines().iter().rev().take(60).rev().cloned().collect::<Vec<_>>().join("
")));
        actions.push(a);
        steps += 1;
        assert!(steps < 100_000, "seed {seed}: game does not end
{}", g.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("
"));
    }
    Played { actions, keys, fingerprint: g.fingerprint(), outcome: g.outcome().unwrap(), turns: g.turn().number }
}

/// A clone continues exactly like the original.
fn check_clone(g: &Game, a: u16, seed: u64) {
    let mut x = g.clone();
    let mut y = g.clone();
    let (mut kx, mut ky) = (Vec::new(), Vec::new());
    x.apply(a, &mut kx).unwrap();
    y.apply(a, &mut ky).unwrap();
    assert_eq!(x.fingerprint(), y.fingerprint(), "seed {seed}: clones diverged");
    assert_eq!(kx, ky);
}

/// `determinize(to_move)` keeps the decider's legal actions and the public
/// state; the resampled world keeps playing without breaking invariants.
fn check_determinize(g: &Game, seed: u64, rng: &mut Rng) {
    let mover = g.to_move().unwrap();
    // The decider (what the search does) and the other player (tests and
    // analysis: its determinize refreshes the decider's decision).
    for viewer in [mover, 1 - mover] {
        let mut d = g.clone();
        d.determinize(viewer, rng);
        if viewer == mover {
            let (mut l1, mut l2) = (Vec::new(), Vec::new());
            g.legal_actions(&mut l1);
            d.legal_actions(&mut l2);
            assert_eq!(l1, l2, "seed {seed}: determinize changed the decider's actions");
        } else {
            assert_eq!(d.to_move(), Some(mover), "seed {seed}: a refresh ran past the decision");
        }
        // Nothing the viewer can observe changes; no card it knows exactly
        // is resampled.
        assert_eq!(
            g.view_of(viewer),
            d.view_of(viewer),
            "seed {seed}: determinize changed the view of P{viewer} (mover P{mover})
{}",
            g.log_lines().iter().rev().take(20).cloned().collect::<Vec<_>>().join("
")
        );
        for (i, o) in g.objs().iter().enumerate() {
            let c = engine::CardIdx(i as u16);
            if g.knows(viewer, c) {
                assert_eq!(o.def, d.obj(c).def, "seed {seed}: a card P{viewer} knows was resampled");
                assert_eq!(g.ident(c), d.ident(c), "seed {seed}: a face-down card P{viewer} knows was resampled");
            }
        }
        d.check_invariants().unwrap();
        // What the search does in that world: try every option.
        check_options_complete(&d, seed, &format!("a world determinized for P{viewer} (mover P{mover})"));
        if viewer == mover {
            check_information_set(g, viewer, seed, rng);
        }
        let mut legal = Vec::new();
        let mut keys = Vec::new();
        for _ in 0..30 {
            if d.is_over() {
                break;
            }
            d.legal_actions(&mut legal);
            d.apply(legal[rng.below(legal.len() as u32) as usize], &mut keys).unwrap();
            d.check_invariants().unwrap_or_else(|e| {
                panic!(
                    "seed {seed} (determinized for P{viewer}): {e}\n{}",
                    d.log_lines().iter().rev().take(40).rev().cloned().collect::<Vec<_>>().join("\n")
                )
            });
        }
    }
}

/// Decisions followed after an option before giving up on reaching priority.
const COMPLETE_STEPS: usize = 64;

/// Options the completion check tried, and the decisions it answered after
/// them (reported by `fuzz_random_games`).
static OPTIONS_TRIED: AtomicU64 = AtomicU64::new(0);
static OPTION_STEPS: AtomicU64 = AtomicU64::new(0);

/// Whether the completion check runs at this decision of the game: not at
/// mulligans and combat declarations (no cast or activation starts there), at
/// half of the frequent priority, payment and target decisions (a castable
/// card stays castable over many of them), at every other one (the rare
/// ones: an effect's "cast it?", a cost option, an X, a sacrifice, ...).
fn complete_now(g: &Game, rng: &mut Rng) -> bool {
    use DecisionKind as K;
    match g.kind() {
        None | Some(K::Mulligan | K::MulliganBottom | K::DeclareAttackers | K::DeclareBlockers | K::AssignDamage | K::AttackTarget) => {
            false
        }
        Some(K::Priority | K::PayMana | K::Target) => rng.below(2) == 0,
        Some(_) => true,
    }
}

/// Every option of the pending decision can be completed (ENGINE.md 6, 18: a
/// cast, an activation or an alternative cost is offered only if it can be
/// completed, so nothing is ever rewound): each legal action, applied to a
/// clone, then the decisions after it answered along one legal path (random,
/// from a generator of its own: the game's sequence is unchanged) until the
/// next priority decision, never panics or fails. The payment's own
/// decisions (targets, X, the cost option, mana sources, sacrifices,
/// discards) all come before the caster's next priority (CR 601.2i, 117.3c).
/// At priority, passing and playing a land are left out (no cost to
/// complete).
fn check_options_complete(g: &Game, seed: u64, world: &str) {
    let Some(d) = g.pending_decision() else { return };
    if d.options.len() < 2 {
        return; // (the game itself takes the one option)
    }
    let priority = d.kind == DecisionKind::Priority;
    for o in &d.options {
        if priority && matches!(o.choice, Choice::Pass | Choice::Play(_) | Choice::PlayFace(_)) {
            continue;
        }
        let a = o.action;
        let path_seed = seed ^ (g.decisions() as u64).wrapping_mul(0x9E37_79B9) ^ ((a as u64) << 40);
        // The option, then one random legal path to the next priority;
        // `describe` keeps the path's descriptions (for a failure).
        let follow = |describe: &mut Option<Vec<String>>| -> Result<(), String> {
            let mut prng = Rng::new(path_seed);
            let mut x = g.clone();
            let (mut keys, mut l) = (Vec::new(), Vec::new());
            x.apply(a, &mut keys)?;
            for _ in 0..COMPLETE_STEPS {
                if x.is_over() || x.kind() == Some(DecisionKind::Priority) {
                    break;
                }
                x.legal_actions(&mut l);
                let b = l[prng.below(l.len() as u32) as usize];
                if let Some(p) = describe {
                    p.push(format!("{b} = {}", x.describe_action(b).unwrap_or_default()));
                }
                OPTION_STEPS.fetch_add(1, Ordering::Relaxed);
                x.apply(b, &mut keys)?;
            }
            Ok(())
        };
        OPTIONS_TRIED.fetch_add(1, Ordering::Relaxed);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| follow(&mut None)));
        let err = match r {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => e,
            Err(p) => p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default(),
        };
        // The same path again, described.
        let mut path = Some(Vec::new());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| follow(&mut path)));
        panic!(
            "seed {seed}: in {world}, option {a} ({}) of {:?} for P{} can't be completed: {err}\npath after it: {:?}\noptions: {:?}",
            g.describe_action(a).unwrap_or_default(),
            d.kind,
            d.player,
            path.unwrap_or_default(),
            g.options_desc()
        );
    }
}

/// Every revealed fact is keyed: two worlds `determinize(viewer)` samples,
/// played with the same actions, stay indistinguishable to the viewer as
/// long as the search would keep them in one node: zero-core keys children
/// by (chance keys, next decider, next legal actions), so equal keys, an
/// equal next decider and equal legal actions must mean an equal view.
fn check_information_set(g: &Game, viewer: u8, seed: u64, rng: &mut Rng) {
    let s = rng.next_u64();
    let mut a = g.clone();
    a.determinize(viewer, &mut Rng::new(s));
    let mut b = g.clone();
    b.determinize(viewer, &mut Rng::new(s ^ 0x5EED));
    let (mut la, mut lb) = (Vec::new(), Vec::new());
    for step in 0..25 {
        a.legal_actions(&mut la);
        b.legal_actions(&mut lb);
        if a.to_move() != b.to_move() || la != lb {
            // Another player's options may depend on its hidden cards (and a
            // decision with one option is answered internally, so who
            // decides next may differ too): the search keeps such worlds in
            // different nodes. The viewer's own options never differ.
            assert!(
                !(a.to_move() == Some(viewer) && b.to_move() == Some(viewer)),
                "seed {seed} step {step}: the viewer's options differ: {:?} / {:?}",
                a.options_desc(),
                b.options_desc()
            );
            return;
        }
        assert_eq!(a.view_of(viewer), b.view_of(viewer), "seed {seed} step {step}: same keys, different views
{}
----
{}", a.log_lines().iter().rev().take(25).rev().cloned().collect::<Vec<_>>().join("
"), b.log_lines().iter().rev().take(25).rev().cloned().collect::<Vec<_>>().join("
"));
        if a.is_over() {
            return;
        }
        let act = la[rng.below(la.len() as u32) as usize];
        let (mut ka, mut kb) = (Vec::new(), Vec::new());
        a.apply(act, &mut ka).unwrap();
        b.apply(act, &mut kb).unwrap();
        if ka != kb {
            return; // different outcomes, told apart by the keys
        }
    }
}

#[test]
fn fuzz_random_games() {
    let n: u64 = std::env::var("FUZZ_GAMES").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    // `FUZZ_FIRST=s` starts at seed s (to rerun a failing seed alone).
    let first: u64 = std::env::var("FUZZ_FIRST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let (mut wins, mut draws, mut turns, mut decisions) = ([0u32; 2], 0u32, 0u64, 0u64);
    for seed in first..first + n {
        let p = std::panic::catch_unwind(|| play(seed, true)).unwrap_or_else(|e| {
            panic!("seed {seed} failed (rerun: FUZZ_FIRST={seed} FUZZ_GAMES=1 FUZZ_LOG=1): {e:?}")
        });
        match p.outcome.winner {
            Some(w) => wins[w as usize] += 1,
            None => draws += 1,
        }
        turns += p.turns as u64;
        decisions += p.actions.len() as u64;
    }
    println!(
        "{n} games: wins {wins:?}, draws {draws}, mean turns {:.1}, mean decisions {:.1}; {} options completed ({} decisions after them)",
        turns as f64 / n as f64,
        decisions as f64 / n as f64,
        OPTIONS_TRIED.load(Ordering::Relaxed),
        OPTION_STEPS.load(Ordering::Relaxed)
    );
}

#[test]
fn same_seed_and_actions_replay_identically() {
    for seed in 1000..1030 {
        let a = play(seed, false);
        // Replay the recorded actions from the same seed.
        let mut g = new_game(seed);
        let mut keys = Vec::new();
        for &x in &a.actions {
            g.apply(x, &mut keys).unwrap();
        }
        assert_eq!(g.fingerprint(), a.fingerprint, "seed {seed}");
        assert_eq!(keys, a.keys, "seed {seed}: chance keys differ");
        assert_eq!(g.outcome(), Some(a.outcome));
        // And the whole random game is reproducible.
        let b = play(seed, false);
        assert_eq!(b.fingerprint, a.fingerprint);
    }
}
