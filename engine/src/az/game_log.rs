//! The EoE sealed game log (ENGINE.md 107.1): every finished real `mtg_eoe` game - self-play and
//! evaluation - as one JSON line, so later deck scoring has each game's two decks, how each was built
//! (the mixed field's mode) and the result. zero-core keeps no per-game record (its result rows stay
//! in memory, its shards hold samples without a game id or seed), so the game records itself.
//!
//! * **Off unless a sink is set** ([`set`]: `mtgz._engine.game_log(path)`; `mtgz.az.run` sets
//!   `<run>/games/games-<pid>.jsonl` for every run it starts, ENGINE.md 107.1). Only sealed games
//!   (`MtgEoeAz`) write; the file is created at the first line, so cube and Delver runs make none.
//! * **Which games**: a [`Record`] is made by `MtgEoeAz::new_game` and carried by the game; a
//!   search's worlds drop it (`determinize`, `determinize_with`: every simulation's world is a
//!   determinized clone, zero-core `tree.rs`), marking the seat whose search dealt it (`searched`: a
//!   network seat; a bot never deals worlds). The live game writes its line at the step that ends
//!   it (over or aborted) and drops the record, so each finished game writes once. A game the driver
//!   stops unfinished, or a resigned one, writes none; a branch game (zero-core `selfplay_branch`, a
//!   clone of a live game) writes its own line with the seed of the game it branched from.
//! * **The line**: `{"v": 1, "spec", "seed", "deck_seed", "eval", "seats": [{"mode", "pool_seed",
//!   "colors", "splash", "lands", "deck": {name: copies}}, x2], "searched", "result", "aborted",
//!   "turns", "time"}`; `spec` keys a spec line written before the first game of that spec in the
//!   file (`{"v": 1, "spec", "sealed", "decks", "eval_seed", "eval_decks", "max_turns",
//!   "hidden_decklists", "deal_samples"}`). `deck_seed` is the seed whose pools and builds the game
//!   plays (`MtgSpec::sealed_seed`: the game's seed, or its evaluation pair's), so the decks can
//!   also be rebuilt from the spec (`mtgz.sealed.games`, which checks they are).
//! * Nothing here touches a game's state: determinism and every encoding are unchanged; a write
//!   that fails panics (a loud failure, never a silently missing record).
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Map, Value};
use zero_search::game::SearchGame;

use super::sealed::{self, builder};
use super::{DeckMode, MtgAz, MtgSpec};

static ON: AtomicBool = AtomicBool::new(false);
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

struct Sink {
    path: PathBuf,
    file: Option<File>,
    /// The spec keys whose spec line this file has.
    specs: Vec<u64>,
}

/// Write the game log to `path` (a JSON-lines file, appended; its directory is created), or stop
/// (None). Games started while it is set write their lines.
pub fn set(path: Option<&str>) -> Result<(), String> {
    let mut s = SINK.lock().unwrap_or_else(|e| e.into_inner());
    *s = match path {
        Some(p) => {
            let path = PathBuf::from(p);
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| format!("game log: {}: {e}", dir.display()))?;
            }
            Some(Sink { path, file: None, specs: Vec::new() })
        }
        None => None,
    };
    ON.store(s.is_some(), Ordering::Relaxed);
    Ok(())
}

/// The log's file, if one is set.
pub fn path() -> Option<String> {
    SINK.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.path.display().to_string())
}

/// A game's record (module docs): made when the game starts, written when it ends.
pub struct Record {
    spec_key: u64,
    spec: Value,
    seed: u64,
    deck_seed: u64,
    eval: Option<u32>,
    seats: [Value; 2],
    /// Bit p: seat p's search dealt a world of this game.
    searched: AtomicU8,
}

/// A deck as {name: copies}.
fn deck_json(deck: &[u16]) -> Value {
    let db = sealed::db();
    let mut m = Map::new();
    for &d in deck {
        let n = &db.def(d).name;
        let c = m.get(n).and_then(|v| v.as_u64()).unwrap_or(0);
        m.insert(n.clone(), json!(c + 1));
    }
    Value::Object(m)
}

impl Record {
    /// The record of a sealed game with spec `spec` and (match) seed `seed`, when the log is on;
    /// else None (and None for a non-sealed spec or a position-lab start).
    pub fn start(spec: &MtgSpec, seed: u64) -> Option<Arc<Record>> {
        if !ON.load(Ordering::Relaxed) || spec.start.is_some() {
            return None;
        }
        let sl = spec.sealed.as_deref()?;
        let deck_seed = spec.sealed_seed(seed);
        let eval = matches!(spec.decks, DeckMode::Eval).then(|| (zero_search::rng::split64(seed) % spec.eval_decks as u64) as u32);
        let mut seats = [Value::Null, Value::Null];
        for (p, out) in seats.iter_mut().enumerate() {
            // (the game's own seats: `MtgAz::new` built them from the same spec and seed)
            let (seat, info) = sl.seat(deck_seed, p as u8).ok()?;
            let b = info.build.as_ref();
            *out = json!({
                "mode": info.mode.name(), "pool_seed": info.pool_seed,
                "colors": b.map(|b| builder::letters(b.colors)), "splash": b.map(|b| builder::letters(b.splash)),
                "lands": seat.deck.iter().filter(|&&d| sealed::db().def(d).is(crate::db::types::LAND)).count(),
                "deck": deck_json(&seat.deck),
            });
        }
        let spec_json = json!({
            "sealed": sl.to_json(), "decks": if eval.is_some() { "eval" } else { "random" }, "eval_seed": spec.eval_seed,
            "eval_decks": spec.eval_decks, "max_turns": spec.max_turns, "hidden_decklists": spec.hidden_decklists,
            "deal_samples": spec.deal_samples,
        });
        let spec_key = spec_json.to_string().bytes().fold(0x6A3E_106Eu64, |h, b| crate::rng::split64(h ^ b as u64));
        Some(Arc::new(Record { spec_key, spec: spec_json, seed, deck_seed, eval, seats, searched: AtomicU8::new(0) }))
    }

    /// Seat `viewer`'s search dealt a world of the game.
    pub fn searched(&self, viewer: usize) {
        self.searched.fetch_or(1 << viewer.min(7), Ordering::Relaxed);
    }

    /// Write the line of the finished game `a` (module docs).
    pub fn finish(&self, a: &MtgAz) {
        let aborted = a.aborted();
        let r = a.result();
        let s = self.searched.load(Ordering::Relaxed);
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        let line = json!({
            "v": 1, "spec": format!("{:016x}", self.spec_key), "seed": self.seed, "deck_seed": self.deck_seed, "eval": self.eval,
            "seats": self.seats, "searched": [s & 1 != 0, s & 2 != 0],
            "result": if aborted { Value::Null } else { json!([r[0], r[1]]) }, "aborted": aborted, "turns": a.rounds(),
            "time": (time * 1000.0).round() / 1000.0,
        });
        let mut g = SINK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sink) = g.as_mut() else { return };
        let mut out = String::new();
        if !sink.specs.contains(&self.spec_key) {
            let mut sp = self.spec.clone();
            sp["v"] = json!(1);
            sp["spec"] = json!(format!("{:016x}", self.spec_key));
            out.push_str(&sp.to_string());
            out.push('\n');
        }
        out.push_str(&line.to_string());
        out.push('\n');
        if sink.file.is_none() {
            let f = OpenOptions::new().create(true).append(true).open(&sink.path);
            sink.file = Some(f.unwrap_or_else(|e| panic!("game log: {}: {e}", sink.path.display())));
        }
        let f = sink.file.as_mut().expect("opened");
        f.write_all(out.as_bytes()).and_then(|_| f.flush()).unwrap_or_else(|e| panic!("game log: {}: {e}", sink.path.display()));
        if !sink.specs.contains(&self.spec_key) {
            sink.specs.push(self.spec_key);
        }
    }
}
