//! Behaviour digests (ENGINE.md 15): a fixed set of games hashed by what a
//! player of the engine can see, so a change meant to keep behaviour (a
//! performance change) can be checked against the recorded digests of the
//! code before it.
//!
//! Per game, the hash covers, decision by decision: the pending decision's
//! kind and player, its legal action ids, the action taken, the chance keys
//! `apply` emitted; every 10th decision the decider's encoding
//! (`encode_features`, bit for bit); every 20th decision a determinized
//! clone for the decider (its legal actions, both players' hand and library
//! identities in order, and five random actions played in it with their
//! legal actions and keys); and the outcome and the turn. None of it
//! depends on the engine's internal representation.
//!
//! Games: 2,000 of uniform random play (seeds 0..2000, random decks of every
//! supported card as in the fuzzer) and 500 of `az::bots::Heuristic` against
//! random play (seeds 0..500, the bot in seat `seed % 2`).
//!
//! `cargo test --release -p engine --test digest -- --ignored --nocapture`
//! compares with `tests/data/digests.txt`; `DIGEST_WRITE=1` records it
//! instead (only when behaviour is meant to change, or for a new baseline).
//! `DIGEST_RANDOM=n` / `DIGEST_HEURISTIC=n` change the game counts (a
//! subset of the recorded games is still compared). `DIGEST_ENCODING=0`
//! leaves the encodings out and prints the combined digest only (compare it
//! across a layout change, run on both sides; the recorded file covers the
//! encodings). `DIGEST_ENCODING=v4` hashes the layout v4 part of each
//! encoding (`encode::v4_projection`) and prints the combined digest only:
//! layout v4's recorded 317b5ba05dc569fb (ENGINE.md 58.11) while every v4
//! float of layout v5 is v4's (ENGINE.md 60.2) and no game plays
//! differently. `DIGEST_ENCODING=v5` hashes the layout v5 part
//! (`encode::v5_projection`): layout v5's recorded full digest while every
//! v5 float of layout v6 is v5's (ENGINE.md 100.5). `DIGEST_ENCODING=v6`
//! hashes the layout v6 part (`encode::v6_projection`, the first 44,212
//! floats): layout v6's recorded full digest while every v6 float of layout
//! v7 is v6's (ENGINE.md 124.2).
//!
//! `DIGEST_IDS=choices` (ENGINE.md 65.11): an id-agnostic trace of the same
//! games, to compare across a change that renames action ids and nothing
//! else (run on both sides): every action id (the legal ones, the one
//! taken, in the game and in the determinized clones) is hashed as the
//! choice it names (`Game::describe_choice`: the card by name and index, the
//! target, the number, ...) instead of the id, and a `KEY_AUTO` chance key
//! without its action (its decision kind and player kept); the encodings are
//! left out (they are hashed ids' neighbours: the list block). It prints the
//! combined digest only.

mod common;

use engine::az::bots::Heuristic;
use engine::az::NUM_FEATURES;
use engine::rng::Rng;
use engine::{Game, GameSpec};

/// FNV-1a over 64-bit words (stable across Rust versions, unlike
/// `DefaultHasher`).
struct H(u64);

impl H {
    fn new() -> H {
        H(0xcbf2_9ce4_8422_2325)
    }
    fn u64(&mut self, x: u64) {
        for b in x.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn slice16(&mut self, xs: &[u16]) {
        self.u64(xs.len() as u64);
        for &x in xs {
            self.u64(x as u64);
        }
    }
    fn slice64(&mut self, xs: &[u64]) {
        self.u64(xs.len() as u64);
        for &x in xs {
            self.u64(x);
        }
    }
}

fn decks(seed: u64) -> (Vec<u16>, Vec<u16>) {
    let db = common::db();
    let mut rng = Rng::new(seed ^ 0xD16E_57);
    (common::random_deck(db, &mut rng), common::random_deck(db, &mut rng))
}

/// `DIGEST_IDS=choices`: actions hashed as the choices they name.
fn by_choice() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("DIGEST_IDS").is_ok_and(|v| v == "choices"))
}

/// Action `a` of the pending decision as hashed: its id, or
/// (`DIGEST_IDS=choices`) a hash of the choice it names.
fn action_word(g: &Game, a: u16) -> u64 {
    if !by_choice() {
        return a as u64;
    }
    let c = g.pending_decision().and_then(|d| d.choice_of(a)).expect("a legal action");
    let mut h = H::new();
    for b in g.describe_choice(c).bytes() {
        h.u64(b as u64);
    }
    h.0
}

/// The chance keys of a transition (`DIGEST_IDS=choices`: a `KEY_AUTO`
/// without its action, ENGINE.md 10).
fn hash_keys(h: &mut H, keys: &[u64]) {
    if !by_choice() {
        h.slice64(keys);
        return;
    }
    h.u64(keys.len() as u64);
    for &k in keys {
        h.u64(if engine::hidden::key_kind(k) == engine::hidden::KEY_AUTO { k & !0xFFFF } else { k });
    }
}

/// The pending decision's kind and player, and the legal actions.
fn hash_decision(h: &mut H, g: &Game, legal: &mut Vec<u16>) {
    g.legal_actions(legal);
    match g.pending_decision() {
        Some(d) => {
            h.u64(d.kind as u64);
            h.u64(d.player as u64);
        }
        None => h.u64(0xFFFF),
    }
    if by_choice() {
        h.u64(legal.len() as u64);
        for &a in legal.iter() {
            h.u64(action_word(g, a));
        }
    } else {
        h.slice16(legal);
    }
}

fn hash_zones(h: &mut H, g: &Game) {
    for p in 0..2u8 {
        let pl = g.player(p);
        h.u64(pl.hand.len() as u64);
        for &c in &pl.hand {
            h.u64(g.obj(c).def as u64);
        }
        h.u64(pl.library.len() as u64);
        for &c in &pl.library {
            h.u64(g.obj(c).def as u64);
        }
    }
}

/// A determinized clone for the decider, played on five random actions.
fn hash_determinized(h: &mut H, g: &Game, seed: u64, step: u32, legal: &mut Vec<u16>, keys: &mut Vec<u64>) {
    let Some(viewer) = g.to_move() else { return };
    let mut d = g.clone();
    d.determinize(viewer, &mut Rng::new(seed.wrapping_mul(0x9E37_79B9) ^ step as u64));
    hash_decision(h, &d, legal);
    hash_zones(h, &d);
    let mut rng = Rng::new(seed ^ (step as u64) << 20);
    for _ in 0..5 {
        if d.is_over() {
            break;
        }
        d.legal_actions(legal);
        let a = legal[rng.below(legal.len() as u32) as usize];
        let w = action_word(&d, a);
        keys.clear();
        d.apply(a, keys).unwrap();
        h.u64(w);
        hash_keys(h, keys);
        hash_decision(h, &d, legal);
    }
}

fn hash_encoding(h: &mut H, g: &Game, buf: &mut [f32]) {
    // `DIGEST_ENCODING=0`: leave the encodings out (the behaviour alone, to
    // compare across a layout change: ENGINE.md 36.9).
    if std::env::var("DIGEST_ENCODING").is_ok_and(|v| v == "0") || by_choice() {
        return;
    }
    let Some(viewer) = g.to_move() else { return };
    buf.fill(0.0);
    g.encode_features(viewer, buf);
    let mut x = H::new();
    // `DIGEST_ENCODING=v4`: hash the layout v4 part of each encoding (the
    // v4 projection, ENGINE.md 60.2): the combined digest is then layout v4's
    // recorded one when every v4 float is what v4 encoded.
    // `DIGEST_ENCODING=v5`: the layout v5 part (the v5 projection, ENGINE.md
    // 100.5): layout v5's recorded digest when every v5 float is what v5
    // encoded.
    let proj = match std::env::var("DIGEST_ENCODING").as_deref() {
        Ok("v4") => {
            static P4: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
            Some(P4.get_or_init(engine::az::encode::v4_projection))
        }
        Ok("v5") => {
            static P5: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
            Some(P5.get_or_init(engine::az::encode::v5_projection))
        }
        // `DIGEST_ENCODING=v6`: the layout v6 part (the first 44,212 floats,
        // ENGINE.md 124.2): layout v6's recorded full digest while every v6
        // float of layout v7 is v6's.
        Ok("v6") => {
            static P6: std::sync::OnceLock<Vec<usize>> = std::sync::OnceLock::new();
            Some(P6.get_or_init(engine::az::encode::v6_projection))
        }
        _ => None,
    };
    if let Some(p) = proj {
        for &i in p {
            x.u64(buf[i].to_bits() as u64);
        }
    } else {
        for &f in buf.iter() {
            x.u64(f.to_bits() as u64);
        }
    }
    h.u64(x.0);
}

/// One game; `bot`: the seat the heuristic plays (else both random).
fn play(seed: u64, bot: Option<u8>, buf: &mut [f32]) -> u64 {
    let (d0, d1) = decks(seed + if bot.is_some() { 1_000_000 } else { 0 });
    let mut g = Game::new(common::db(), GameSpec::default(), [&d0, &d1], seed).unwrap();
    let mut rng = Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 3);
    let mut heur = Heuristic::new(seed ^ 0xB07);
    let (mut legal, mut legal2, mut keys, mut keys2) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut h = H::new();
    let mut step = 0u32;
    while !g.is_over() {
        hash_decision(&mut h, &g, &mut legal);
        if step % 10 == 0 {
            hash_encoding(&mut h, &g, buf);
        }
        if step % 20 == 7 {
            hash_determinized(&mut h, &g, seed, step, &mut legal2, &mut keys2);
        }
        let a = if bot.is_some() && g.to_move() == bot {
            heur.choose(&g, &legal)
        } else {
            legal[rng.below(legal.len() as u32) as usize]
        };
        let w = action_word(&g, a);
        keys.clear();
        g.apply(a, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        h.u64(w);
        hash_keys(&mut h, &keys);
        step += 1;
        assert!(step < 100_000, "seed {seed}: game does not end");
    }
    let o = g.outcome().unwrap();
    h.u64(o.winner.map_or(9, |w| w as u64));
    h.u64(o.reason as u64);
    h.u64(g.turn().number as u64);
    h.u64(step as u64);
    h.0
}

fn env_n(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

#[test]
#[ignore]
fn behaviour_digests() {
    let n_random = env_n("DIGEST_RANDOM", 2000);
    let n_heur = env_n("DIGEST_HEURISTIC", 500);
    let mut buf = vec![0.0f32; NUM_FEATURES];
    let mut lines = Vec::new();
    let t = std::time::Instant::now();
    for seed in 0..n_random {
        lines.push(format!("r {seed} {:016x}", play(seed, None, &mut buf)));
    }
    for seed in 0..n_heur {
        lines.push(format!("h {seed} {:016x}", play(seed, Some((seed % 2) as u8), &mut buf)));
    }
    let mut all = H::new();
    for l in &lines {
        for b in l.bytes() {
            all.u64(b as u64);
        }
    }
    println!(
        "{n_random} random + {n_heur} heuristic games in {:.1} s; combined digest {:016x}",
        t.elapsed().as_secs_f64(),
        all.0
    );
    // `DIGEST_LINES=<file>`: every game's digest written there, in any mode
    // (to count the games a change alters in a mode the recorded file
    // doesn't hold: run on both sides and compare).
    if let Ok(p) = std::env::var("DIGEST_LINES") {
        std::fs::write(&p, lines.join("\n") + "\n").unwrap();
    }
    if std::env::var("DIGEST_ENCODING").is_ok_and(|v| v == "0" || v == "v4" || v == "v5" || v == "v6") || by_choice() {
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/digests.txt");
    if std::env::var("DIGEST_WRITE").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        println!("wrote {}", path.display());
        return;
    }
    let recorded = std::fs::read_to_string(&path).expect("tests/data/digests.txt (record with DIGEST_WRITE=1)");
    let recorded: std::collections::HashMap<&str, &str> = recorded
        .lines()
        .filter_map(|l| l.rsplit_once(' '))
        .collect();
    let mut differ = Vec::new();
    for l in &lines {
        let (game, digest) = l.rsplit_once(' ').unwrap();
        match recorded.get(game) {
            Some(&r) if r == digest => {}
            Some(_) => differ.push(game.to_string()),
            None => panic!("game {game} has no recorded digest"),
        }
    }
    assert!(
        differ.is_empty(),
        "{} of {} games differ from the recorded digests; first: {:?}",
        differ.len(),
        lines.len(),
        &differ[..differ.len().min(10)]
    );
}
