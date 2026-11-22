//! Behaviour digests beyond the cube table (ENGINE.md 148). `digest.rs` hashes 2,500 best-of-one games
//! on the cube's table; none of them plays constructed play's table, a best-of-three match, its
//! sideboarding, the constructed prior's hidden 60s, the EoE table with its layout v6 rows, or
//! zero-core's driver (the searches, the belief and deck deals, the evaluation cache, the records).
//! Four digests here, each compared with its recorded file in `tests/data/`:
//!
//! * `constructed_digests` (`digests_constructed.txt`): matches of the Delver best-of-three preset's
//!   spec (`delver-mirror-bo3`, `tests/data/digest_specs.json`: post-sideboard games only, one
//!   episode per game) - 150 of random play and 50 of the heuristic (`MtgAz`'s bot, which keeps its
//!   deck) against random play; 50 of the same spec from game 1 (`start_game` 1: game 1's mains,
//!   then sideboarding); 50 with a seat keeping its deck (`keep_deck`, the yardstick's); and 270
//!   best-of-one games of the real 60s (`data/decks/`: Delver, Doomsday, Sneak and Show, every
//!   pairing and both seats by `seed % 9`), 90 of them the heuristic against random play.
//! * `eoe_digests` (`digests_eoe.txt`): `mtg_eoe` games of the EoE presets' sealed spec (`eoe-1`:
//!   arena pools, the mixed field `best_share` 0.5, hidden decklists dealt by the sealed prior) - 100
//!   random, 40 heuristic against random - and 20 of run 2's spec, the deck choice before the game
//!   (`eoe-2`: 32 candidates, played at random).
//! * `metagame_digests` (`digests_metagame.txt`, ENGINE.md 176): the three-deck Legacy metagame preset's spec
//!   (`legacy-meta3`, `tests/data/digest_specs.json`'s `meta3`: best-of-one, the mains, each game's pairing drawn from
//!   its seed) - 180 games of random play, 45 of `MtgAz`'s heuristic against random play -, 45 matches of its
//!   best-of-three (the 75s, one episode per game) and 18 post-sideboard matches of its mirrors (`"start_game": 2`,
//!   weights on the diagonal); each line names the game's pairing.
//! * `driver_digest` (`digests_driver.txt`): zero-core's `Driver<MtgAz>` on the Delver best-of-three
//!   preset's spec with a self-play seat as the live run's (fewer simulations: 16, 8 in a fast search,
//!   4 for a minor decision; 2 groups of 8 games; the completed-Q target, the evaluation cache, root-view leaves with view
//!   samples, the belief and deck tilts with their mix, resignation) and an instantaneous evaluator (a
//!   peaked prior on the first option, value 0.5): every batch (its rows, options, tails, overrides,
//!   kinds and games), every sample and every result of a fixed number of batches, hashed in the order
//!   the driver hands them out (the review's harness, `perf.md` section 4). A record's features are
//!   hashed as their non-zero (index, value) pairs, so a sparse record hashes as a dense one. The
//!   hash does not depend on the thread count (`DIGEST_DRIVER_THREADS`, default 4).
//!
//! Per game or match the first two hash, decision by decision: the decider, the pending decision's
//! kind and player (a sideboarding's decider and kind, a deck choice), the legal actions, the action
//! taken and the chance keys; every 10th decision the decider's encoding (`mtg`: the whole layout v8
//! row - head, tail tokens, pointer overrides, ENGINE.md 232; `DIGEST_ENCODING=v7`: layout v7's, the
//! combined digests printed only; `mtg_eoe`: its layout v6 floats); every 20th the
//! decider's auxiliary targets and a world `determinize` deals it (its decision, both players' hand
//! and library identities in order, both decks, the other player's encoding there, five random
//! actions played in it); every 40th a world dealt by a random belief (`determinize_with`: the hand's
//! tilt every other time, the deck's where decks are hidden) with its identities and decks; each
//! game's end (the result, the outcome, the turn), each next episode, and the match's results.
//!
//! `cargo test --release -p engine --test digest_formats -- --ignored --nocapture` compares (the four
//! run at once); `DIGEST_WRITE=1` records instead - only when behaviour is meant to change, stating
//! the old -> new combined digests. `DIGEST_ENCODING=0` leaves the encodings out of the first two and
//! prints their combined digests only (compare them across an encoding-only change, run on both
//! sides); the driver's hash always covers its rows. `DIGEST_LINES=<dir>` writes every line there too.
//! Each test prints one line `<name>: ... combined digest <16 hex>`.

use engine::az::constructed::Decklist;
use engine::az::eoe::MtgEoeAz;
use engine::az::{identity, MtgAz, MtgSpec};
use engine::rng::Rng;
use zero_search::driver::{Driver, DriverCfg, KeyKind, LeafView, NnSeat, PolicyTarget, SeatCfg};
use zero_search::game::{Belief, Next, SearchGame};

// The module's allocator (`py/src/lib.rs`): the driver digest is also a benchmark of the driver
// as `mtgz._engine` runs it (ENGINE.md 148).
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// FNV-1a over 64-bit words (as `digest.rs`).
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
    fn slice<T: Copy + Into<u64>>(&mut self, xs: &[T]) {
        self.u64(xs.len() as u64);
        for &x in xs {
            self.u64(x.into());
        }
    }
    fn f32s(&mut self, xs: &[f32]) {
        self.u64(xs.len() as u64);
        for &x in xs {
            self.u64(x.to_bits() as u64);
        }
    }
    fn i64s(&mut self, xs: &[i64]) {
        self.u64(xs.len() as u64);
        for &x in xs {
            self.u64(x as u64);
        }
    }
    fn i32s(&mut self, xs: &[i32]) {
        self.u64(xs.len() as u64);
        for &x in xs {
            self.u64(x as u32 as u64);
        }
    }
}

/// The specs of `tests/data/digest_specs.json` (copies of the presets', `tests/test_digest_specs.py`).
fn spec_json(name: &str) -> String {
    let all: serde_json::Value = serde_json::from_str(include_str!("data/digest_specs.json")).unwrap();
    all[name].to_string()
}

fn encodings_on() -> bool {
    !std::env::var("DIGEST_ENCODING").is_ok_and(|v| v == "0")
}

/// `DIGEST_ENCODING=v7` (ENGINE.md 232): `mtg`'s layout v7 rows hashed instead of layout v8's, the combined
/// digests printed only (layout v7's recorded files of main e445ba27 while v8 only adds segments).
fn layout_v7() -> bool {
    std::env::var("DIGEST_ENCODING").is_ok_and(|v| v == "v7")
}

/// A game of the `MtgAz` family, as the digests read it.
trait Digested: SearchGame + Clone {
    fn az(&self) -> &MtgAz;
    /// `viewer`'s encoding hashed: `mtg`'s whole layout v8 row (`mtg_v8`'s; v7's with `DIGEST_ENCODING=v7`),
    /// `mtg_eoe`'s layout v6 floats.
    fn hash_encoding(&self, viewer: usize, h: &mut H);
}

impl Digested for MtgAz {
    fn az(&self) -> &MtgAz {
        self
    }
    fn hash_encoding(&self, viewer: usize, h: &mut H) {
        let row = engine::az::tail_encode::Row::of_az_layout(self, viewer, !layout_v7());
        let mut x = H::new();
        x.f32s(&row.head);
        x.u64(row.tail.len() as u64);
        for (sg, sl, v) in &row.tail {
            x.u64(*sg as u64);
            x.u64(*sl as u64);
            x.f32s(v);
        }
        x.u64(row.ov.len() as u64);
        for &(o, t) in &row.ov {
            x.u64(o as u64);
            x.u64(t as u64);
        }
        h.u64(x.0);
    }
}

impl Digested for MtgEoeAz {
    fn az(&self) -> &MtgAz {
        &self.0
    }
    fn hash_encoding(&self, viewer: usize, h: &mut H) {
        let mut v = vec![0.0f32; <MtgEoeAz as SearchGame>::NUM_FEATURES];
        self.encode(viewer, &mut v);
        let mut x = H::new();
        x.f32s(&v);
        h.u64(x.0);
    }
}

/// The decider, its pending decision (a game's, a sideboarding's, a deck choice) and the legal actions.
fn hash_decision<G: Digested>(h: &mut H, g: &G, legal: &mut Vec<u16>) {
    g.legal_actions(legal);
    h.u64(g.to_move().map_or(0xFFFF, |p| p as u64));
    let a = g.az();
    if let Some(sb) = a.sideboarding() {
        h.u64(0x5B);
        h.u64(sb.decider() as u64);
        h.u64(sb.decision.kind as u64);
    } else if a.m.choice.is_some() {
        h.u64(0xDC);
    } else {
        match a.g.pending_decision() {
            Some(d) => {
                h.u64(d.kind as u64);
                h.u64(d.player as u64);
            }
            None => h.u64(0xFFFE),
        }
    }
    h.slice(&legal[..]);
}

/// Both players' hand and library identities in order, and both decks (a match's counts).
fn hash_zones(h: &mut H, a: &MtgAz) {
    for p in 0..2u8 {
        let pl = a.g.player(p);
        h.u64(pl.hand.len() as u64);
        for &c in &pl.hand {
            h.u64(a.g.obj(c).def as u64);
        }
        h.u64(pl.library.len() as u64);
        for &c in &pl.library {
            h.u64(a.g.obj(c).def as u64);
        }
        h.slice(&a.m.decks[p as usize][..]);
    }
}

/// A world `determinize` deals the decider, played on five random actions.
fn hash_world<G: Digested>(h: &mut H, g: &G, seed: u64, step: u32, legal: &mut Vec<u16>, keys: &mut Vec<u64>) {
    let Some(viewer) = g.to_move() else { return };
    let mut d = g.clone();
    d.determinize(viewer, &mut zero_search::rng::Rng::new(seed.wrapping_mul(0x9E37_79B9) ^ step as u64));
    hash_decision(h, &d, legal);
    hash_zones(h, d.az());
    if encodings_on() {
        d.hash_encoding(1 - viewer, h);
    }
    let mut rng = Rng::new(seed ^ (step as u64) << 20);
    for _ in 0..5 {
        if d.to_move().is_none() {
            break;
        }
        d.legal_actions(legal);
        let a = legal[rng.below(legal.len() as u32) as usize];
        keys.clear();
        d.step(a, keys).unwrap();
        h.u64(a as u64);
        h.slice(&keys[..]);
        hash_decision(h, &d, legal);
    }
}

/// A world `determinize_with` deals the decider by a random belief (both targets' predictions from a
/// generator of its own): the hand's tilt 1 every other time, the deck's 1 where decks are hidden.
fn hash_belief_world<G: Digested>(h: &mut H, g: &G, seed: u64, step: u32) {
    let Some(viewer) = g.to_move() else { return };
    let mut pr = Rng::new(seed ^ (step as u64).wrapping_mul(0x2545_F491) ^ 0xBE11EF);
    let mut seg = || {
        let mut p: Vec<f32> = (0..identity::D).map(|_| ((pr.below(1000) + 1) as f32).powi(3)).collect();
        let s: f32 = p.iter().sum();
        p.iter_mut().for_each(|x| *x /= s);
        p
    };
    let mut pred = seg();
    pred.extend(seg());
    let hand = if (step / 40) % 2 == 0 { 1.0 } else { 0.0 };
    let deck = if g.az().deals_hidden_decks() { 1.0 } else { 0.0 };
    let b = Belief { pred, tilt: hand, clip: 20.0, tilts: vec![0.0, deck] };
    let mut d = g.clone();
    d.determinize_with(viewer, &mut zero_search::rng::Rng::new(pr.next_u64()), &b);
    h.u64(0xBE11);
    hash_zones(h, d.az());
}

/// One game or match of `G`: `bot` = the seat `G`'s heuristic plays (else random play for both).
fn play<G: Digested>(spec: &G::Spec, seed: u64, bot: Option<usize>) -> u64 {
    let mut g = G::new_game(spec, 2, seed).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    let mut heur = bot.map(|_| G::make_bot(spec, "heuristic", seed ^ 0xB07).expect("the heuristic"));
    let mut rng = Rng::new(seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 3);
    let aux_w = G::aux_width();
    let (mut legal, mut legal2, mut keys, mut keys2) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut h = H::new();
    let mut step = 0u32;
    loop {
        let Some(p) = g.to_move() else {
            // a game's (an episode's) end
            let a = g.az();
            h.u64(0xE0D);
            h.f32s(&g.result()[..2]);
            if let Some(o) = a.g.outcome() {
                h.u64(o.winner.map_or(9, |w| w as u64));
                h.u64(o.reason as u64);
            }
            h.u64(a.g.turn().number as u64);
            h.u64(a.m.game_no as u64);
            if !g.chained() {
                break;
            }
            match g.next_episode(None).unwrap_or_else(|e| panic!("seed {seed}: {e}")) {
                Next::Continue(n) => {
                    g = n;
                    h.u64(0xE9);
                    continue;
                }
                Next::Over(r) => {
                    h.f32s(&r[..2]);
                    break;
                }
            }
        };
        hash_decision(&mut h, &g, &mut legal);
        if step % 10 == 0 && encodings_on() {
            g.hash_encoding(p, &mut h);
        }
        if step % 20 == 7 {
            if aux_w > 0 {
                let mut aux = vec![0.0f32; aux_w];
                g.aux_target(p, &mut aux);
                h.f32s(&aux);
            }
            hash_world(&mut h, &g, seed, step, &mut legal2, &mut keys2);
        }
        if step % 40 == 27 {
            hash_belief_world(&mut h, &g, seed, step);
        }
        let a = match (&mut heur, bot) {
            (Some(b), Some(s)) if s == p => b.choose(&g, &legal),
            _ => legal[rng.below(legal.len() as u32) as usize],
        };
        keys.clear();
        g.step(a, &mut keys).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        h.u64(a as u64);
        h.slice(&keys[..]);
        step += 1;
        assert!(step < 300_000, "seed {seed}: the match does not end");
    }
    let m = &g.az().m;
    h.slice(&m.results[..]);
    h.u64(m.game_no as u64);
    h.u64(step as u64);
    h.0
}

/// Compare `lines` with `tests/data/<file>` (or record them with `DIGEST_WRITE=1`); print the combined digest.
fn finish(name: &str, file: &str, what: &str, lines: &[String], t: std::time::Instant) {
    let mut all = H::new();
    for l in lines {
        for b in l.bytes() {
            all.u64(b as u64);
        }
    }
    println!("{name}: {what} in {:.1} s; combined digest {:016x}", t.elapsed().as_secs_f64(), all.0);
    if let Ok(dir) = std::env::var("DIGEST_LINES") {
        std::fs::write(std::path::Path::new(&dir).join(file), lines.join("\n") + "\n").unwrap();
    }
    if (!encodings_on() || layout_v7()) && name != "driver" {
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(file);
    if std::env::var("DIGEST_WRITE").is_ok() {
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        println!("wrote {}", path.display());
        return;
    }
    let recorded = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e} (record with DIGEST_WRITE=1)", path.display()));
    let recorded: Vec<(&str, &str)> = recorded.lines().filter_map(|l| l.rsplit_once(' ')).collect();
    let mut differ = Vec::new();
    for l in lines {
        let (k, d) = l.rsplit_once(' ').unwrap();
        match recorded.iter().find(|(r, _)| *r == k) {
            Some(&(_, r)) if r == d => {}
            Some(_) => differ.push(k.to_string()),
            None => panic!("{name}: {k} has no recorded digest in {file}"),
        }
    }
    assert_eq!(recorded.len(), lines.len(), "{name}: {file} records {} lines, the test made {}", recorded.len(), lines.len());
    assert!(differ.is_empty(), "{name}: {} of {} lines differ from {file}; first: {:?}", differ.len(), lines.len(), &differ[..differ.len().min(10)]);
}

/// The real 60s on main (`data/decks/`): 0 Delver, 1 Doomsday, 2 Sneak and Show.
fn real_main(k: u64) -> Vec<String> {
    match k {
        0 => Decklist::izzet_delver().main_names(),
        1 => Decklist::legacy_doomsday().main_names(),
        _ => Decklist::parse(include_str!("../../data/decks/sneak_show_8014220.json")).unwrap().main_names(),
    }
}

#[test]
#[ignore]
fn constructed_digests() {
    let t = std::time::Instant::now();
    let preset: serde_json::Value = serde_json::from_str(&spec_json("delver_bo3")).unwrap();
    let with = |f: &dyn Fn(&mut serde_json::Value)| -> MtgSpec {
        let mut v = preset.clone();
        f(&mut v);
        MtgSpec::parse(&v.to_string()).unwrap()
    };
    let post = with(&|_| {});
    assert_eq!(post.start_game, 2, "the preset's spec is post-sideboard (ENGINE.md 130)");
    let full = with(&|v| {
        v.as_object_mut().unwrap().remove("start_game");
    });
    let keep = [0, 1].map(|s| with(&|v| v["keep_deck"] = serde_json::json!([s])));
    let mut lines = Vec::new();
    for seed in 0..150 {
        lines.push(format!("p {seed} {:016x}", play::<MtgAz>(&post, seed, None)));
    }
    for seed in 0..50 {
        lines.push(format!("ph {seed} {:016x}", play::<MtgAz>(&post, 1000 + seed, Some((seed % 2) as usize))));
    }
    for seed in 0..50 {
        lines.push(format!("f {seed} {:016x}", play::<MtgAz>(&full, 2000 + seed, None)));
    }
    for seed in 0..50 {
        lines.push(format!("k {seed} {:016x}", play::<MtgAz>(&keep[(seed % 2) as usize], 3000 + seed, None)));
    }
    // best-of-one games of the real 60s: every pairing, both seats (`seed % 9`)
    let mains = [real_main(0), real_main(1), real_main(2)];
    let bo1 = |seed: u64| {
        let (p0, p1) = ((seed % 9) / 3, seed % 3);
        let v = serde_json::json!({"max_turns": 60, "constructed": true, "decks": {"p0": mains[p0 as usize], "p1": mains[p1 as usize]}});
        MtgSpec::parse(&v.to_string()).unwrap()
    };
    for seed in 0..180 {
        lines.push(format!("l {seed} {:016x}", play::<MtgAz>(&bo1(seed), 4000 + seed, None)));
    }
    for seed in 0..90 {
        lines.push(format!("lh {seed} {:016x}", play::<MtgAz>(&bo1(seed), 5000 + seed, Some((seed / 9 % 2) as usize))));
    }
    finish("constructed", "digests_constructed.txt", "300 best-of-three matches and 270 best-of-one games", &lines, t);
}

/// `legacy-meta3`'s spec (ENGINE.md 176) as the digest plays it: best-of-one as recorded (`meta3`), or best-of-three
/// with each deck's sideboard (`bo3`), `extra` merged in.
fn meta3(bo3: bool, extra: serde_json::Value) -> MtgSpec {
    let mut v: serde_json::Value = serde_json::from_str(&spec_json("meta3")).unwrap();
    if bo3 {
        v["format"] = serde_json::json!("bo3");
        for k in 0..3 {
            let name = v["metagame"]["decks"][k]["name"].as_str().unwrap().to_string();
            let d = Decklist::parse(&std::fs::read_to_string(format!("{}/../data/decks/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
            let side: Vec<String> = d.sideboard.iter().flat_map(|(n, c)| std::iter::repeat_n(c.clone(), *n as usize)).collect();
            v["metagame"]["decks"][k]["sideboard"] = serde_json::json!(side);
        }
    }
    for (k, x) in extra.as_object().unwrap() {
        // (`weights`: the metagame's)
        if k == "weights" {
            v["metagame"]["weights"] = x.clone();
        } else {
            v[k] = x.clone();
        }
    }
    MtgSpec::parse(&v.to_string()).unwrap()
}

#[test]
#[ignore]
fn metagame_digests() {
    let t = std::time::Instant::now();
    let bo1 = meta3(false, serde_json::json!({}));
    let bo3 = meta3(true, serde_json::json!({"game_episodes": true}));
    // (the mirrors only: start_game 2 is for mirrors, ENGINE.md 147.4)
    let post = meta3(true, serde_json::json!({"game_episodes": true, "start_game": 2, "weights": [[1, 0, 0], [0, 1, 0], [0, 0, 1]]}));
    let cell = |s: &MtgSpec, seed: u64| s.pairing(seed).map(|p| format!("{}{}", p.decks[0], p.decks[1])).unwrap();
    let mut lines = Vec::new();
    for seed in 0..180 {
        lines.push(format!("mg {seed} {} {:016x}", cell(&bo1, seed), play::<MtgAz>(&bo1, seed, None)));
    }
    for seed in 0..45 {
        lines.push(format!("mh {seed} {} {:016x}", cell(&bo1, 1000 + seed), play::<MtgAz>(&bo1, 1000 + seed, Some((seed % 2) as usize))));
    }
    for seed in 0..45 {
        lines.push(format!("mm {seed} {} {:016x}", cell(&bo3, 2000 + seed), play::<MtgAz>(&bo3, 2000 + seed, None)));
    }
    for seed in 0..18 {
        let c = cell(&post, 3000 + seed);
        assert_eq!(c[..1], c[1..], "a mirror");
        lines.push(format!("mp {seed} {c} {:016x}", play::<MtgAz>(&post, 3000 + seed, None)));
    }
    finish("metagame", "digests_metagame.txt", "225 best-of-one games and 63 best-of-three matches", &lines, t);
}

#[test]
#[ignore]
fn eoe_digests() {
    let t = std::time::Instant::now();
    let spec = <MtgEoeAz as SearchGame>::parse_spec(&spec_json("eoe")).unwrap();
    let choice = <MtgEoeAz as SearchGame>::parse_spec(&spec_json("eoe_choice")).unwrap();
    let mut lines = Vec::new();
    for seed in 0..100 {
        lines.push(format!("e {seed} {:016x}", play::<MtgEoeAz>(&spec, seed, None)));
    }
    for seed in 0..40 {
        lines.push(format!("eh {seed} {:016x}", play::<MtgEoeAz>(&spec, 1000 + seed, Some((seed % 2) as usize))));
    }
    for seed in 0..20 {
        lines.push(format!("ec {seed} {:016x}", play::<MtgEoeAz>(&choice, 2000 + seed, None)));
    }
    finish("eoe", "digests_eoe.txt", "160 mtg_eoe games", &lines, t);
}

/// The live run's self-play seat (the review's harness, `perf.md`), with fewer simulations.
fn selfplay_seat() -> NnSeat {
    let mut s = NnSeat::eval(0, 16);
    s.selfplay = true;
    s.sims_fast = Some(8);
    s.p_full = 0.25;
    s.sims_minor = Some(4);
    s.temp_moves = 60;
    s.policy_target = PolicyTarget::CompletedQ { c_visit: 50.0, c_scale: 0.3, prior_w: 1.0 };
    s.eval_cache = 8192;
    s.leaf_value_view = LeafView::Root;
    s.view_samples = 0.5;
    s.belief_tilt = 1.0;
    s.belief_mix = 0.25;
    s.belief_tilts = vec![("deck".to_string(), 1.0)];
    s.resign_t = 0.02;
    s.resign_k = 3;
    s.resign_noresign = 0.1;
    s
}

/// Batches the driver digest plays (and records a checkpoint line every `DRIVER_EVERY`, so a change shows
/// the first stretch of batches it alters).
const DRIVER_BATCHES: u64 = 20_000;
const DRIVER_EVERY: u64 = 1_000;

#[test]
#[ignore]
fn driver_digest() {
    let t = std::time::Instant::now();
    let threads = std::env::var("DIGEST_DRIVER_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let cfg = DriverCfg {
        spec: spec_json("delver_bo3"),
        seats: vec![SeatCfg::Nn(selfplay_seat())],
        games: vec![vec![0, 0]],
        keys: vec![("m".into(), KeyKind::Nn)],
        concurrency: 8,
        seed: 11,
        record: true,
        threads,
        groups: 2,
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
    };
    let mut d = Driver::<MtgAz>::new(cfg).unwrap();
    d.resume(1_000_000, 0).unwrap();
    let w = MtgAz::aux_width();
    let mut h = H::new();
    let (mut batches, mut rows, mut samples, mut results) = (0u64, 0u64, 0u64, 0u64);
    let mut lines = Vec::new();
    let mut first = (0u64, 0u64);
    while batches < DRIVER_BATCHES {
        let b = d.next_batch(true).unwrap().expect("a rolling phase goes on");
        h.u64(b.key as u64);
        h.u64(b.b as u64);
        h.f32s(&b.x);
        h.i32s(&b.opt_off);
        h.i32s(&b.opt_action);
        h.i32s(&b.player);
        h.i64s(&b.gid);
        h.slice(&b.extra[..]);
        h.i64s(&b.xs_off);
        h.slice(&b.xs_idx[..]);
        h.f32s(&b.xs_val);
        h.i64s(&b.t_off);
        h.slice(&b.t_seg[..]);
        h.slice(&b.t_slot[..]);
        h.f32s(&b.t_val);
        h.i64s(&b.ov_off);
        h.i32s(&b.ov_opt);
        h.i64s(&b.ov_tok);
        // the evaluator: a peaked prior (the first option 0.85, as a trained network's top), value 0.5
        let mut pri = Vec::with_capacity(b.opt_action.len());
        for r in 0..b.b {
            let k = (b.opt_off[r + 1] - b.opt_off[r]) as usize;
            for j in 0..k {
                pri.push(if k == 1 { 1.0 } else if j == 0 { 0.85 } else { 0.15 / (k - 1) as f32 });
            }
        }
        let mut v = vec![0.0f32; b.b * 8];
        for r in 0..b.b {
            v[r * 8] = 0.5;
            v[r * 8 + 1] = 0.5;
        }
        let nb = b.extra.iter().filter(|&&e| e == zero_search::driver::ROW_BELIEF).count();
        let aux = vec![1.0 / identity::D as f32; nb * w];
        d.submit_aux(b.id, &pri, &v, &vec![0.0; b.b * 8], &aux).unwrap();
        batches += 1;
        rows += b.b as u64;
        for r in d.take_samples() {
            samples += 1;
            h.u64(0x5A);
            let nz: Vec<(usize, u32)> = r.x.iter().enumerate().filter(|(_, v)| v.to_bits() != 0).map(|(i, v)| (i, v.to_bits())).collect();
            h.u64(nz.len() as u64);
            for (i, v) in nz {
                h.u64(i as u64);
                h.u64(v as u64);
            }
            h.u64(r.player as u64);
            h.u64(r.n_players as u64);
            h.slice(&r.opts[..]);
            h.f32s(&r.pi);
            h.f32s(&r.z);
            h.f32s(&r.q);
            h.f32s(&r.sc);
            h.u64(r.net as u64);
            h.f32s(&r.aux);
            h.slice(&r.tail.seg[..]);
            h.slice(&r.tail.slot[..]);
            h.f32s(&r.tail.val);
            h.u64(r.ov.len() as u64);
            for &(o, t) in &r.ov {
                h.u64(o as u64);
                h.u64(t as u64);
            }
        }
        for r in d.take_results() {
            results += 1;
            h.u64(0x7E);
            h.u64(r.gid as u64);
            h.u64(r.n as u64);
            h.u64(r.over as u64);
            h.u64(r.aborted as u64);
            h.f32s(&r.result);
            h.f32s(&r.scores);
            h.u64(r.steps);
            h.u64(r.decisions as u64);
            h.u64(r.rounds as u64);
            h.u64(r.samples as u64);
            h.u64(r.resign.as_ref().map_or(0, |x| 1 + x.resigned as u64 + 2 * x.noresign as u64));
            h.u64(r.episodes.len() as u64);
            for e in &r.episodes {
                h.u64(e.over as u64);
                h.u64(e.aborted as u64);
                h.f32s(&e.result);
                h.f32s(&e.margins);
                h.u64(e.steps);
                h.u64(e.decisions as u64);
                h.u64(e.samples as u64);
            }
        }
        if first.0 == 0 && samples > 0 {
            first.0 = batches;
        }
        if first.1 == 0 && results > 0 {
            first.1 = batches;
        }
        if batches % DRIVER_EVERY == 0 {
            lines.push(format!("b {batches} {:016x}", h.0));
        }
    }
    let st = d.stats();
    println!(
        "driver ({threads} threads, 2 groups of 8 games): {batches} batches, {rows} rows, {samples} samples (the first at batch {}), {results} results (the first at batch {}); sims {}, leaves {}, cache hits {}, view rows {}, belief rows {}, busy {:.1} s ({:.1} us a simulation)",
        first.0,
        first.1,
        st.sims,
        st.leaves,
        st.cache_hits,
        st.view_rows,
        st.belief_rows,
        st.busy_s,
        st.busy_s * 1e6 / st.sims.max(1) as f64
    );
    assert!(samples > 0 && results > 0, "the driver digest saw {samples} samples and {results} results: it must cover both");
    lines.push(format!("driver {:016x}", h.0));
    finish("driver", "digests_driver.txt", &format!("{batches} batches, {samples} samples, {results} results"), &lines, t);
}
