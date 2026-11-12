//! The EoE sealed game log (ENGINE.md 107.1): every finished real `mtg_eoe` game - self-play and
//! evaluation - as one JSON line, so later deck scoring has each game's two decks, how each was built
//! (the mixed field's mode) and the result. zero-core keeps no per-game record (its result rows stay
//! in memory, its shards hold samples without a game id or seed), so the game records itself.
//!
//! * **Off unless a sink is set** ([`set`]: `mtgz._engine.game_log(path)`; `mtgz.az.run` sets
//!   `<run>/games/games-<pid>.jsonl` for every run it starts, ENGINE.md 107.1). Only sealed games
//!   (`MtgEoeAz`) write; the file is created at the first line, so cube and Delver runs make none.
//! * **Which games**: a [`Record`] (the spec and seeds) is made by `MtgEoeAz::new_game` and carried by the game; a
//!   search's worlds drop it (`determinize`, `determinize_with`: every simulation's world is a
//!   determinized clone, zero-core `tree.rs`), marking the seat whose search dealt it (`searched`: a
//!   network seat; a bot never deals worlds). The live game writes its line at the step that ends
//!   it (over or aborted) and drops the record, so each finished game writes once. A game the driver
//!   stops unfinished, or a resigned one, writes none; a branch game (zero-core `selfplay_branch`, a
//!   clone of a live game) writes its own line with the seed of the game it branched from.
//! * **The line**: `{"v": 1, "spec", "seed", "deck_seed", "eval", "seats": [{"mode", "candidate",
//!   "pool_seed", "colors", "splash", "lands", "deck": {name: copies}}, x2], "searched", "result",
//!   "aborted", "turns", "time"}` - each seat's deck and how it was made read from the finished match
//!   (`MatchState::deck`, `MatchState::how`: a deck choice's chosen deck, ENGINE.md 107.2); `spec` keys a spec line written before the first game of that spec in the
//!   file (`{"v": 1, "spec", "sealed", "decks", "eval_seed", "eval_decks", "max_turns",
//!   "hidden_decklists", "deal_samples"}`). `deck_seed` is the seed whose pools and builds the game
//!   plays (`MtgSpec::sealed_seed`: the game's seed, or its evaluation pair's), so the decks can
//!   also be rebuilt from the spec (`mtgz.sealed.games`, which checks they are).
//! * **Version 2: the exact replay and the draws** (ENGINE.md 107.3). The live game's record keeps its trace
//!   (`Trace`, behind a mutex: never copied with the game's clones, read and written by the live game
//!   only): every action applied to it (`MtgEoeAz::step`, the deck choice's included), and per seat its
//!   draws (turn, kind - 0 a hand drawn before the first turn, the opening hand or a mulligan's, 1 the draw
//!   step's, 2 an effect's -, the card), the cards it put on the bottom after a mulligan, its mulligans, and
//!   the cards it played or cast (turn, kind - 0 a land played, 1 a spell cast, from Priority or the
//!   graveyard list -, the card). The draws come from the live game's own chance keys (`KEY_DRAW`: the true
//!   card, the live game has no observer), the plays from the decision's choice of the action. The line
//!   adds `"actions"` (the actions as little-endian u16, base64) and `"n_actions"`, and per seat `"draws"`,
//!   `"bottom"`, `"mulligans"`, `"plays"` (a card as its index among the sorted names of the seat's
//!   `"deck"`, else its name). The spec line is the game's whole spec as far as a game depends on it, so
//!   `MtgSpec::parse(spec line)` + `new_game(.., seed)` + the actions replay the game exactly (tested).
//! * **A deck chosen by an evaluator** (ENGINE.md 112): its seat's `"mode"` is `"chosen"` with its `"candidate"`, and
//!   the seat adds `"evaluator"` (the sha256 of the evaluator file that chose it; the spec line names the file and
//!   its sha256) and `"score"` (its score by it).
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
use crate::db::types;
use crate::decision::{Choice, DecisionKind};
use crate::hidden::{key_kind, key_payload, key_player, KEY_DRAW};
use crate::ids::DefId;

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
    /// Bit p: seat p's search dealt a world of this game.
    searched: AtomicU8,
    /// The live game's trace (version 2): its actions, draws and plays.
    trace: Mutex<Trace>,
}

/// The live game's trace (module docs, version 2).
#[derive(Default)]
struct Trace {
    actions: Vec<u16>,
    seats: [SeatTrace; 2],
}

/// One seat's draws (turn, kind, card), bottomed cards, mulligans, plays (turn, kind, card).
#[derive(Default)]
struct SeatTrace {
    draws: Vec<(u32, u8, DefId)>,
    bottom: Vec<DefId>,
    mulligans: u32,
    plays: Vec<(u32, u8, DefId)>,
}

/// What a step's record needs from the state before it (`Record::before`).
pub struct Before {
    turn: u32,
    draw_step_draws: [u32; 2],
    /// The decider's play of the action: (seat, kind, card) for a land played (0) or a spell cast (1).
    play: Option<(u8, u8, DefId)>,
    /// The action's mulligan answer: (seat, true = a mulligan taken) or a card put on the bottom.
    mulligan: Option<u8>,
    bottom: Option<(u8, DefId)>,
}

/// Standard base64 (RFC 4648, with padding) of `b`.
fn base64(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for (i, k) in [18, 12, 6, 0].into_iter().enumerate() {
            s.push(if i <= c.len() { A[(n >> k & 63) as usize] as char } else { '=' });
        }
    }
    s
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
        // (the spec as a game depends on it: parsed back, it makes the same game from the same seed)
        let mut spec_json = json!({
            "sealed": sl.to_json(), "decks": if eval.is_some() { "eval" } else { "random" }, "eval_seed": spec.eval_seed,
            "eval_decks": spec.eval_decks, "max_turns": spec.max_turns, "hidden_decklists": spec.hidden_decklists,
            "deal_samples": spec.deal_samples, "mulligans": spec.mulligans, "deck_tilt_ess": spec.deck_tilt_ess.0,
        });
        if let Some(c) = spec.chooser {
            spec_json["chooser"] = json!(c);
        }
        let spec_key = spec_json.to_string().bytes().fold(0x6A3E_106Eu64, |h, b| crate::rng::split64(h ^ b as u64));
        let trace = Mutex::new(Trace::default());
        Some(Arc::new(Record { spec_key, spec: spec_json, seed, deck_seed, eval, searched: AtomicU8::new(0), trace }))
    }

    /// Seat `viewer`'s search dealt a world of the game.
    pub fn searched(&self, viewer: usize) {
        self.searched.fetch_or(1 << viewer.min(7), Ordering::Relaxed);
    }

    /// What the step of action `act` from `a` (the live game, before it) will need (version 2).
    pub fn before(&self, a: &MtgAz, act: u16) -> Before {
        let g = &a.g;
        let mut b = Before { turn: g.turn().number, draw_step_draws: [g.player(0).draw_step_draws, g.player(1).draw_step_draws], play: None, mulligan: None, bottom: None };
        if let Some(d) = a.pending_decision() {
            let p = d.player;
            match (d.kind, d.choice_of(act)) {
                (DecisionKind::Priority | DecisionKind::ChooseGraveyardCard, Some(Choice::Play(c))) => b.play = Some((p, 0, g.obj(c).def)),
                (DecisionKind::Priority | DecisionKind::ChooseGraveyardCard, Some(Choice::Cast(c))) => b.play = Some((p, 1, g.obj(c).def)),
                (DecisionKind::Mulligan, Some(Choice::Yes)) => b.mulligan = Some(p),
                (DecisionKind::MulliganBottom, Some(Choice::Card(c))) => b.bottom = Some((p, g.obj(c).def)),
                _ => {}
            }
        }
        b
    }

    /// The step of action `act` happened: `keys` its chance keys, `a` the live game after it (version 2).
    pub fn after(&self, a: &MtgAz, act: u16, b: Before, keys: &[u64]) {
        let g = &a.g;
        let turn = g.turn().number;
        let mut t = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        t.actions.push(act);
        if let Some((p, k, d)) = b.play {
            t.seats[p as usize].plays.push((b.turn, k, d));
        }
        if let Some(p) = b.mulligan {
            t.seats[p as usize].mulligans += 1;
        }
        if let Some((p, d)) = b.bottom {
            t.seats[p as usize].bottom.push(d);
        }
        // the draw step's draws: the counter's increase (from 0 in a new turn); the step's first draws of that seat
        let mut step_draws = [0u32; 2];
        for p in 0..2 {
            let now = g.player(p as u8).draw_step_draws;
            step_draws[p] = if turn != b.turn { now } else { now.saturating_sub(b.draw_step_draws[p]) };
        }
        for &k in keys {
            if key_kind(k) != KEY_DRAW {
                continue;
            }
            let p = key_player(k) as usize & 1;
            let kind = if turn == 0 {
                0
            } else if step_draws[p] > 0 {
                step_draws[p] -= 1;
                1
            } else {
                2
            };
            t.seats[p].draws.push((turn, kind, key_payload(k) as DefId));
        }
    }

    /// Write the line of the finished game `a` (module docs).
    pub fn finish(&self, a: &MtgAz) {
        let aborted = a.aborted();
        let r = a.result();
        let s = self.searched.load(Ordering::Relaxed);
        let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        // each seat's deck as the match holds it (the chosen one after a deck choice), and how it was made
        let t = self.trace.lock().unwrap_or_else(|e| e.into_inner());
        let db = sealed::db();
        let seats: Vec<Value> = (0..2u8)
            .map(|p| {
                let deck = a.m.deck(p);
                let how = a.m.how[p as usize];
                let dj = deck_json(&deck);
                // a card as its index among the deck's sorted names, else its name
                let names: Vec<&String> = dj.as_object().map(|m| m.keys().collect()).unwrap_or_default();
                let card = |d: DefId| {
                    let n = &db.def(d).name;
                    names.iter().position(|x| *x == n).map_or_else(|| json!(n), |i| json!(i))
                };
                let st = &t.seats[p as usize];
                let mut seat = json!({
                    "mode": how.map(|h| h.mode.name()), "candidate": how.and_then(|h| h.candidate), "pool_seed": how.and_then(|h| h.pool_seed),
                    "colors": how.map(|h| builder::letters(h.colors)), "splash": how.map(|h| builder::letters(h.splash)),
                    "lands": deck.iter().filter(|&&d| db.def(d).is(types::LAND)).count(),
                    "draws": st.draws.iter().map(|&(tn, k, d)| json!([tn, k, card(d)])).collect::<Vec<_>>(),
                    "bottom": st.bottom.iter().map(|&d| card(d)).collect::<Vec<_>>(),
                    "mulligans": st.mulligans,
                    "plays": st.plays.iter().map(|&(tn, k, d)| json!([tn, k, card(d)])).collect::<Vec<_>>(),
                    "deck": dj,
                });
                // (a deck chosen by an evaluator, ENGINE.md 112: the evaluator file's sha256 and the deck's score)
                if let Some(h) = how.and_then(|h| h.evaluator) {
                    seat["evaluator"] = json!(super::sealed::scorer::hex(&h));
                    seat["score"] = json!(how.and_then(|h| h.score).map(f64::from_bits));
                }
                seat
            })
            .collect();
        let bytes: Vec<u8> = t.actions.iter().flat_map(|a| a.to_le_bytes()).collect();
        let line = json!({
            "v": 2, "spec": format!("{:016x}", self.spec_key), "seed": self.seed, "deck_seed": self.deck_seed, "eval": self.eval,
            "seats": seats, "searched": [s & 1 != 0, s & 2 != 0], "n_actions": t.actions.len(), "actions": base64(&bytes),
            "result": if aborted { Value::Null } else { json!([r[0], r[1]]) }, "aborted": aborted, "turns": a.rounds(),
            "time": (time * 1000.0).round() / 1000.0,
        });
        let mut g = SINK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(sink) = g.as_mut() else { return };
        let mut out = String::new();
        if !sink.specs.contains(&self.spec_key) {
            let mut sp = self.spec.clone();
            sp["v"] = json!(2);
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
