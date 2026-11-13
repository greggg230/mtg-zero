//! Tree reuse after the other player's pass keeps every world of the observation (ENGINE.md 149; Fable review
//! 2026-11-12, hidden.md "auto_single and the opponent's hidden options").
//!
//! `auto_single` answers the other player's priority in the engine when `PASS` is its only option and keys it
//! (`KEY_AUTO`, section 10); in a world where it holds a castable instant it is asked instead, and the search's tree
//! holds its node there. The reuse of before took the live key's child alone: only the worlds where the opponent had
//! no alternative (a fact of its hidden hand). zero-core's observable answers (docs/search.md 3.18, `MtgAz::ANSWERS`)
//! follow the real line by the observation - a pass, forced or chosen - and merge every node it reaches.
//!
//! The measure: at live transitions of heuristic play (some random moves) where the mover decides again after the
//! other player's pass, the mover's search of its decision (every walk through the real move, uniform priors, an even
//! value) and the walks whose own path observed the real line - generated from each walk's path, independently of the
//! tree's matching - against the walks the reused root holds, before (`advance_tagged`, the live key's child) and
//! after (`line_start` + `line_advance`); and the same where the other player was asked and passed (the reuse of
//! before dropped the tree).

use std::time::{Duration, Instant};

use engine::az::{MtgAz, MtgSpec};
use engine::decision::act::PASS;
use engine::hidden::{key_kind, key_player, KEY_AUTO};
use engine::rng::Rng;
use zero_search::game::{KeyObs, SearchGame, MAX_SEATS};
use zero_search::tree::{tag_of, transition_with, Scratch, Sim, Tree, Walk};

const W: Walk = Walk { c_puct: 1.0, chance_k: 0, margin_w: 0.0 };

fn env(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn legal(g: &MtgAz) -> Vec<u16> {
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    v
}

/// Seat `p`'s search of its decision in `g` with every walk through root option `a`: uniform priors, an even value.
/// -> the tree and, per walk that went through `p`'s next decision, what that walk observed on its way there (the
/// keys of its nodes and the other seat's answers it took, `answer_obs`) and that node.
fn search_through(g: &MtgAz, a: u16, sims: usize, seed: u64) -> (Tree<MtgAz>, Vec<(Vec<u64>, u32)>) {
    let p = g.to_move().unwrap();
    let la = legal(g);
    let ia = la.iter().position(|&x| x == a).unwrap() as usize;
    let mut rng = zero_search::rng::Rng::new(seed);
    let mut t = Tree::new(g, p, &la, true, &mut rng);
    let mut scratch = g.clone();
    let mut sc = Scratch::default();
    let mut win = [0.0; MAX_SEATS];
    win[0] = 0.5;
    win[1] = 0.5;
    let mut walks = Vec::new();
    for _ in 0..sims {
        let r = t.simulate_with(g, &mut scratch, &mut rng, &W, &mut sc, |t: &Tree<MtgAz>, node, _| (node == t.root).then_some(ia)).unwrap();
        if sc.path.len() > 1 {
            let mut obs = Vec::new();
            for &(n, b) in &sc.path[1..] {
                obs.extend_from_slice(t.key_of(n));
                if t.player(n) == p {
                    walks.push((obs, n));
                    break;
                }
                match MtgAz::answer_obs(t.player(n), t.tag(n), t.acts(n)[b as usize]) {
                    Some(o) => obs.push(o),
                    None => break,
                }
            }
        }
        if let Sim::Leaf { node, .. } = r {
            let k = t.acts(node).len();
            let pri = vec![1.0 / k.max(1) as f32; k];
            t.expand(node, &pri, &win);
            t.backup(&sc.path, &win, &win);
        }
    }
    (t, walks)
}

/// The viewer's line of a real transition's keys (`key_obs`): None where an answer is opaque (the driver drops).
fn line_of(keys: &[u64], viewer: usize, out: &mut Vec<u64>) -> Option<()> {
    for &k in keys {
        if MtgAz::key_obs(k, viewer) == KeyObs::Opaque {
            return None;
        }
        out.push(k);
    }
    Some(())
}

#[derive(Default, Debug)]
struct Tally {
    transitions: usize,
    opaque: usize,
    observed: u64,
    before: u64,
    after: u64,
    split: usize,
    asked: usize,
    asked_observed: u64,
    asked_after: u64,
    t_before: Duration,
    t_after: Duration,
}

fn measure(json: &str, label: &str, games: u64, every: usize, sims: usize) -> Tally {
    let spec = MtgSpec::parse(json).unwrap();
    let mut n = Tally::default();
    let mut seen = 0usize;
    for seed in 0..games {
        let mut g = MtgAz::new(&spec, seed).unwrap();
        let mut rng = Rng::new(seed ^ 0xA070);
        let mut h = engine::az::bots::Heuristic::new(seed);
        let (mut key, mut l) = (Vec::new(), Vec::new());
        for step in 0..4000u64 {
            let Some(p) = g.to_move() else { break };
            let la = legal(&g);
            let a = if rng.below(4) == 0 { la[rng.below(la.len() as u32) as usize] } else { h.choose(&g.g, &la) };
            let before = g.clone();
            transition_with(&mut g, a, None, &mut key, &mut l, |_| true).unwrap();
            let o = 1 - p;
            let np = g.to_move();
            let forced = key.iter().any(|&k| key_kind(k) == KEY_AUTO && key_player(k) as usize == o);
            let asked = np == Some(o) && l.contains(&PASS);
            if la.len() < 2 || !((forced && np == Some(p)) || asked) {
                continue;
            }
            seen += 1;
            if seen % every != 0 {
                continue;
            }
            let (t, walks) = search_through(&before, a, sims, seed * 100_003 + step);
            let mut line = Vec::new();
            if line_of(&key, p, &mut line).is_none() {
                n.opaque += 1;
                continue;
            }
            // the other player asked: it passes (a clone; the game goes on with the real choice), maybe again
            let mut next = g.clone();
            let mut ok = true;
            let mut segs: Vec<(u64, Vec<u64>)> = Vec::new();
            if asked {
                let (mut k2, mut l2) = (Vec::new(), Vec::new());
                while next.to_move() == Some(o) {
                    if !legal(&next).contains(&PASS) {
                        ok = false;
                        break;
                    }
                    // (a pass of priority; another question's PASS is not observable alike: the driver drops)
                    let Some(ans) = MtgAz::answer_obs(o, tag_of(&next), PASS) else {
                        ok = false;
                        break;
                    };
                    line.push(ans);
                    transition_with(&mut next, PASS, None, &mut k2, &mut l2, |_| true).unwrap();
                    if line_of(&k2, p, &mut line).is_none() {
                        ok = false;
                        break;
                    }
                    segs.push((ans, k2.clone()));
                }
                if !ok || next.to_move() != Some(p) {
                    continue;
                }
            }
            let nl = legal(&next);
            let ntag = tag_of(&next);
            let observed = walks.iter().filter(|(obs, nd)| *obs == line && t.acts(*nd) == nl.as_slice() && t.tag(*nd) == ntag).count() as u64;
            // the line's reuse (the driver's: its own move, then the other player's answers)
            let mut x = t.clone();
            let t0 = Instant::now();
            let reused = x.line_start(a, &key) && segs.iter().all(|(ans, k)| x.line_answer(*ans, k)) && x.line_advance(p, &nl, ntag);
            let dt = t0.elapsed();
            let got = if reused { x.tot(x.root) as u64 } else { 0 };
            assert_eq!(got, observed, "{label} seed {seed} step {step}: the reused root holds {got} walks, {observed} observed the line");
            if asked {
                n.asked += 1;
                n.asked_observed += observed;
                n.asked_after += got;
                continue;
            }
            let mut y = t.clone();
            let t1 = Instant::now();
            let old = if y.advance_tagged(a, &key, p, &nl, ntag) { y.tot(y.root) as u64 } else { 0 };
            n.t_before += t1.elapsed();
            n.t_after += dt;
            n.transitions += 1;
            n.observed += observed;
            n.before += old;
            n.after += got;
            n.split += (old < observed) as usize;
        }
    }
    let pct = |a: u64, b: u64| 100.0 * a as f64 / b.max(1) as f64;
    println!(
        "{label}: {} forced-pass transitions measured ({} opaque skipped): {} walks observed the line; reused before {} ({:.1}%, left out {:.1}%), after {} ({:.1}%); {} transitions had chosen-pass worlds; advance {:.1} us/transition before, {:.1} after",
        n.transitions,
        n.opaque,
        n.observed,
        n.before,
        pct(n.before, n.observed),
        100.0 - pct(n.before, n.observed),
        n.after,
        pct(n.after, n.observed),
        n.split,
        n.t_before.as_secs_f64() * 1e6 / n.transitions.max(1) as f64,
        n.t_after.as_secs_f64() * 1e6 / n.transitions.max(1) as f64,
    );
    println!("{label}: {} asked-and-passed transitions: {} walks observed the line, reused after {} (before: the tree was dropped)", n.asked, n.asked_observed, n.asked_after);
    n
}

#[test]
fn the_reused_tree_keeps_every_world_of_the_other_players_pass() {
    let (games, every, sims) = (env("AUTOPASS_GAMES", 12) as u64, env("AUTOPASS_EVERY", 4), env("AUTOPASS_SIMS", 160));
    let cube = measure(r#"{"max_turns": 30, "eval_decks": 16, "eval_seed": 1}"#, "cube bo1", games, every, sims);
    let main = engine::az::constructed::Decklist::izzet_delver().main_names();
    let json = serde_json::json!({"max_turns": 30, "constructed": true, "decks": {"p0": main, "p1": main}}).to_string();
    let delver = measure(&json, "delver mirror bo1", games, every, sims);
    for (label, n) in [("cube", &cube), ("delver", &delver)] {
        assert!(n.transitions > 0 && n.asked > 0, "{label}: {n:?}");
        assert_eq!(n.after, n.observed, "{label}");
        assert_eq!(n.asked_after, n.asked_observed, "{label}");
    }
    // the search did reach worlds where the other player was asked and passed, which the reuse of before left out
    assert!(cube.split + delver.split > 0 && cube.before + delver.before < cube.observed + delver.observed);
}
