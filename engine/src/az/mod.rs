//! The zero-core game `mtg` (ENGINE.md 21): one game of Magic between two
//! 40-card decks of supported cards, as a `zero_search::SearchGame`.
//!
//! * Decisions are the engine's (`pending_decision`); actions are its
//!   decision-relative ids (`NUM_ACTIONS` = 1024, ENGINE.md 11).
//! * `step` = `Game::apply` with its chance keys (ENGINE.md 10);
//!   `force_chance` replays a key in another world; `determinize` resamples
//!   what the viewer does not know.
//! * `result`: 1 / 0, a draw (including the turn cap) 0.5 each; `margin`:
//!   the life difference / 20 in [-1, 1].
//! * `encode`: `encode.rs` (13,532 floats, no hidden information).
//! * Decks: `decks.rs` (random two-colour decks per game, or the fixed
//!   evaluation set, or given lists).
//! * Bots: `random` (zero-core) and `heuristic` (`bots.rs`).

pub mod bots;
pub mod decks;
pub mod encode;

use crate::db::CardDb;
use crate::game::{Game, GameSpec};
use crate::ids::DefId;
use crate::rng::Rng;
use serde_json::Value;
use zero_search::game::{Bot, SearchGame, MAX_SEATS};

pub use decks::DeckGen;
pub use encode::NUM_FEATURES;

/// Where a game's decks come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckMode {
    /// Two new random decks per game (`decks::generate`, from the game seed).
    Random,
    /// A pair of the fixed evaluation set (`decks::eval_pair`), chosen by the
    /// game seed.
    Eval,
    /// The same two decklists every game (seat 0, seat 1).
    Lists(Vec<DefId>, Vec<DefId>),
}

/// The spec of the `mtg` game (JSON; every key optional):
///
/// ```json
/// {"max_turns": 60, "mulligans": true, "decks": "random", "lands": [16, 17],
///  "eval_decks": 16, "eval_seed": 1}
/// ```
///
/// `decks`: `"random"` (new decks per game from the game seed), `"eval"`
/// (one of the `eval_decks` pairs of the evaluation set `eval_seed`, chosen
/// by the game seed) or `{"p0": [names], "p1": [names]}`. `max_turns`: the
/// game is a draw after this many turns (both players' turns count).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MtgSpec {
    pub max_turns: u32,
    pub mulligans: bool,
    pub decks: DeckMode,
    pub gen: DeckGen,
    pub eval_decks: u32,
    pub eval_seed: u64,
}

impl Default for MtgSpec {
    fn default() -> Self {
        MtgSpec {
            max_turns: 60,
            mulligans: true,
            decks: DeckMode::Random,
            gen: DeckGen::default(),
            eval_decks: 16,
            eval_seed: 1,
        }
    }
}

impl MtgSpec {
    pub fn parse(json: &str) -> Result<MtgSpec, String> {
        let mut s = MtgSpec::default();
        if json.trim().is_empty() {
            return Ok(s);
        }
        let v: Value = serde_json::from_str(json).map_err(|e| format!("mtg spec: {e}"))?;
        let obj = v.as_object().ok_or("mtg spec: not a JSON object")?;
        let db = CardDb::cube();
        for (k, x) in obj {
            let int = || x.as_u64().ok_or_else(|| format!("mtg spec: {k} must be a non-negative integer"));
            match k.as_str() {
                "max_turns" => s.max_turns = int()? as u32,
                "mulligans" => s.mulligans = x.as_bool().ok_or("mtg spec: mulligans must be a boolean")?,
                "eval_decks" => s.eval_decks = (int()? as u32).max(1),
                "eval_seed" => s.eval_seed = int()?,
                "lands" => {
                    let a = x.as_array().filter(|a| a.len() == 2).ok_or("mtg spec: lands must be [min, max]")?;
                    let (lo, hi) = (a[0].as_u64().unwrap_or(0), a[1].as_u64().unwrap_or(0));
                    if lo < 10 || hi < lo || hi > 24 {
                        return Err(format!("mtg spec: lands {lo}..{hi} out of range"));
                    }
                    s.gen.min_lands = lo as u8;
                    s.gen.max_lands = hi as u8;
                }
                "decks" => {
                    s.decks = match x {
                        Value::String(m) if m == "random" => DeckMode::Random,
                        Value::String(m) if m == "eval" => DeckMode::Eval,
                        Value::Object(o) => {
                            let list = |key: &str| -> Result<Vec<DefId>, String> {
                                let a = o
                                    .get(key)
                                    .and_then(|v| v.as_array())
                                    .ok_or_else(|| format!("mtg spec: decks.{key} must be a list of card names"))?;
                                a.iter()
                                    .map(|n| {
                                        let n = n.as_str().ok_or("mtg spec: card names are strings")?;
                                        let id = db.expect_id(n)?;
                                        if db.def(id).token {
                                            return Err(format!("mtg spec: {n} is a token"));
                                        }
                                        Ok(id)
                                    })
                                    .collect()
                            };
                            DeckMode::Lists(list("p0")?, list("p1")?)
                        }
                        _ => return Err(format!("mtg spec: unknown decks {x}")),
                    }
                }
                other => return Err(format!("mtg spec: unknown key {other:?}")),
            }
        }
        if s.max_turns == 0 {
            return Err("mtg spec: max_turns must be positive".into());
        }
        Ok(s)
    }

    /// The two decks of a game with this seed.
    pub fn decks(&self, seed: u64) -> [Vec<DefId>; 2] {
        let db = CardDb::cube();
        match &self.decks {
            DeckMode::Random => {
                let mut rng = Rng::new(seed ^ 0x00DE_C50F_2A11);
                [decks::generate(db, &mut rng, &self.gen), decks::generate(db, &mut rng, &self.gen)]
            }
            DeckMode::Eval => {
                let i = (zero_search::rng::split64(seed) % self.eval_decks as u64) as u32;
                decks::eval_pair(db, &self.gen, self.eval_seed, i)
            }
            DeckMode::Lists(a, b) => [a.clone(), b.clone()],
        }
    }

    /// The same spec with the fixed evaluation decks (zero-core uses one
    /// spec for self-play and evaluation; `mtgz.az.run` switches evaluation
    /// matches to this).
    pub fn eval_json(json: &str) -> Result<String, String> {
        let mut v: Value = if json.trim().is_empty() { Value::Object(Default::default()) } else {
            serde_json::from_str(json).map_err(|e| format!("mtg spec: {e}"))?
        };
        let o = v.as_object_mut().ok_or("mtg spec: not a JSON object")?;
        if o.get("decks").is_none_or(|d| d == "random") {
            o.insert("decks".into(), Value::String("eval".into()));
        }
        Ok(v.to_string())
    }
}

/// One game of Magic for zero-core.
#[derive(Clone)]
pub struct MtgAz {
    pub g: Game,
}

impl MtgAz {
    pub fn new(spec: &MtgSpec, seed: u64) -> Result<MtgAz, String> {
        let [d0, d1] = spec.decks(seed);
        let gspec = GameSpec { max_turns: spec.max_turns, mulligans: spec.mulligans, ..GameSpec::default() };
        Ok(MtgAz { g: Game::new(CardDb::cube(), gspec, [&d0, &d1], seed)? })
    }
}

/// The `heuristic` bot as a zero-core `Bot`.
struct HeuristicBot(bots::Heuristic);

impl Bot<MtgAz> for HeuristicBot {
    fn choose(&mut self, g: &MtgAz, legal: &[u16]) -> u16 {
        self.0.choose(&g.g, legal)
    }
}

impl SearchGame for MtgAz {
    type Spec = MtgSpec;
    const NAME: &'static str = "mtg";
    const NUM_ACTIONS: usize = crate::decision::NUM_ACTIONS;
    const NUM_FEATURES: usize = encode::NUM_FEATURES;
    const MIN_PLAYERS: usize = 2;
    const MAX_PLAYERS: usize = 2;

    fn parse_spec(json: &str) -> Result<MtgSpec, String> {
        MtgSpec::parse(json)
    }

    fn new_game(spec: &MtgSpec, players: usize, seed: u64) -> Result<Self, String> {
        if players != 2 {
            return Err(format!("mtg: {players} players (two only)"));
        }
        MtgAz::new(spec, seed)
    }

    fn num_players(&self) -> usize {
        2
    }

    fn to_move(&self) -> Option<usize> {
        self.g.to_move().map(|p| p as usize)
    }

    fn legal_actions(&self, out: &mut Vec<u16>) {
        self.g.legal_actions(out);
    }

    fn step(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        self.g.apply(a, chance)
    }

    fn force_chance(&mut self, forced: &[u64]) {
        self.g.force_chance(forced);
    }

    fn result(&self) -> [f32; MAX_SEATS] {
        let mut r = [0.0; MAX_SEATS];
        let v = if self.g.is_over() { self.g.result_values() } else { [0.5, 0.5] };
        r[0] = v[0];
        r[1] = v[1];
        r
    }

    fn margin(&self, seat: usize) -> f32 {
        let me = self.g.life(seat as u8).max(0) as f32;
        let them = self.g.life(1 - seat as u8).max(0) as f32;
        ((me - them) / 20.0).clamp(-1.0, 1.0)
    }

    fn score(&self, seat: usize) -> f32 {
        self.g.life(seat as u8) as f32
    }

    fn determinize(&mut self, viewer: usize, rng: &mut zero_search::rng::Rng) {
        let mut r = Rng::new(rng.next_u64());
        self.g.determinize(viewer as u8, &mut r);
    }

    fn encode(&self, viewer: usize, out: &mut [f32]) {
        self.g.encode_features(viewer as u8, out);
    }

    fn rounds(&self) -> u32 {
        self.g.turn().number
    }

    fn bot_names() -> &'static [&'static str] {
        &["heuristic"]
    }

    fn make_bot(_spec: &MtgSpec, name: &str, seed: u64) -> Option<Box<dyn Bot<Self>>> {
        match name {
            "heuristic" => Some(Box::new(HeuristicBot(bots::Heuristic::new(seed)))),
            _ => None,
        }
    }
}
