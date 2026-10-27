//! The zero-core game `mtg` (ENGINE.md 21): one match of Magic between two
//! players with 40-card decks of supported cards, as a
//! `zero_search::SearchGame`: one game (`"format": "bo1"`, the default) or
//! a best-of-three with sideboarding (`"bo3"`, ENGINE.md 36: `matches.rs`).
//!
//! * Decisions are the engine's (`pending_decision`); actions are its
//!   decision-relative ids (`NUM_ACTIONS` = 1024, ENGINE.md 11).
//! * `step` = `Game::apply` with its chance keys (ENGINE.md 10);
//!   `force_chance` replays a key in another world; `determinize` resamples
//!   what the viewer does not know.
//! * `result`: 1 / 0, a draw (including the turn cap) 0.5 each (a match:
//!   by game wins); `margin`: the life difference / 20 in [-1, 1] (a match:
//!   mostly the game wins, ENGINE.md 36.6).
//! * `encode`: `encode.rs` (13,532 floats, no hidden information).
//! * Decks: `decks.rs` (random two-colour decks per game, or the fixed
//!   evaluation set, or given lists).
//! * Bots: `random` (zero-core) and `heuristic` (`bots.rs`).
//!
//! The game `team_draft` (DESIGN.md 2.3, ENGINE.md 41): `draft.rs` (the event,
//! deckbuilding, the deferred terminal), `draft_encode.rs` (layout
//! `draft_v1`), `draft_bots.rs` (`consensus`, heuristic-played matches).

pub mod bots;
pub mod decks;
pub mod draft;
pub mod draft_bots;
pub mod draft_encode;
pub mod encode;
pub mod identity;
pub mod matches;
pub mod pools;
pub mod sample;

use crate::db::CardDb;
use crate::game::{Game, GameSpec};
use crate::ids::DefId;
use crate::rng::Rng;
use serde_json::Value;
use zero_search::game::{Bot, SearchGame, MAX_SEATS};

pub use decks::DeckGen;
pub use encode::NUM_FEATURES;
pub use matches::{Format, MatchState, Seat};

/// Where a game's decks come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckMode {
    /// Two new random decks per game (`decks::generate`, from the game seed).
    Random,
    /// A pair of the fixed evaluation set (`decks::eval_pair`), chosen by the
    /// game seed.
    Eval,
    /// The same two decklists every game (seat 0, seat 1), and their
    /// sideboards (ENGINE.md 35).
    Lists(Vec<DefId>, Vec<DefId>),
    ListsWithSideboards([(Vec<DefId>, Vec<DefId>); 2]),
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
/// by the game seed) or `{"p0": [names], "p1": [names]}`, optionally with
/// `"p0_sideboard"` / `"p1_sideboard"` lists (ENGINE.md 35; random and
/// evaluation decks have generated sideboards). `max_turns`: the game is a
/// draw after this many turns (both players' turns count). `chooser`: the
/// seat that chooses who takes the first turn (CR 103.1; default: chosen at
/// random each game). M8b (ENGINE.md 36): `format`: `"bo1"` (one game, the
/// default) or `"bo3"` (a best-of-three match with sideboarding); `pools`:
/// random and evaluation decks come from generated card pools of 45 cards
/// built into decks by the heuristic deckbuilder, the rest of the pool the
/// sideboard (`pools.rs`; default: true for `"bo3"`, false for `"bo1"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MtgSpec {
    pub max_turns: u32,
    pub mulligans: bool,
    pub decks: DeckMode,
    pub gen: DeckGen,
    pub eval_decks: u32,
    pub eval_seed: u64,
    pub chooser: Option<u8>,
    pub format: Format,
    pub pools: bool,
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
            chooser: None,
            format: Format::Bo1,
            pools: false,
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
        let mut pools = None;
        for (k, x) in obj {
            let int = || x.as_u64().ok_or_else(|| format!("mtg spec: {k} must be a non-negative integer"));
            match k.as_str() {
                "max_turns" => s.max_turns = int()? as u32,
                "format" => {
                    s.format = match x.as_str() {
                        Some("bo1") => Format::Bo1,
                        Some("bo3") => Format::Bo3,
                        _ => return Err(format!("mtg spec: format must be \"bo1\" or \"bo3\", not {x}")),
                    }
                }
                "pools" => pools = Some(x.as_bool().ok_or("mtg spec: pools must be a boolean")?),
                "mulligans" => s.mulligans = x.as_bool().ok_or("mtg spec: mulligans must be a boolean")?,
                "eval_decks" => s.eval_decks = (int()? as u32).max(1),
                "eval_seed" => s.eval_seed = int()?,
                "chooser" => {
                    let c = int()?;
                    if c > 1 {
                        return Err("mtg spec: chooser must be 0 or 1".into());
                    }
                    s.chooser = Some(c as u8);
                }
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
                            for key in o.keys() {
                                if !["p0", "p1", "p0_sideboard", "p1_sideboard"].contains(&key.as_str()) {
                                    return Err(format!("mtg spec: unknown decks key {key:?}"));
                                }
                            }
                            let side = |key: &str| if o.contains_key(key) { list(key) } else { Ok(Vec::new()) };
                            let (s0, s1) = (side("p0_sideboard")?, side("p1_sideboard")?);
                            if s0.is_empty() && s1.is_empty() {
                                DeckMode::Lists(list("p0")?, list("p1")?)
                            } else {
                                DeckMode::ListsWithSideboards([(list("p0")?, s0), (list("p1")?, s1)])
                            }
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
        s.pools = pools.unwrap_or(s.format == Format::Bo3);
        Ok(s)
    }

    /// The two decks of a game with this seed (the decks of [`pools`](Self::pools)).
    pub fn decks(&self, seed: u64) -> [Vec<DefId>; 2] {
        let [(a, _), (b, _)] = self.pools(seed);
        [a, b]
    }

    /// The two decks of a game with this seed and their sideboards
    /// (ENGINE.md 35; with `pools`, the deckbuilder's decks and the rest of
    /// the pools, ENGINE.md 36.4).
    pub fn pools(&self, seed: u64) -> [(Vec<DefId>, Vec<DefId>); 2] {
        self.seats(seed).map(|s| (s.deck, s.sideboard))
    }

    /// The first game's decks, sideboards and first picks of a match with
    /// this seed (ENGINE.md 36.4).
    pub fn seats(&self, seed: u64) -> [Seat; 2] {
        let db = CardDb::cube();
        let built = |p: pools::Pool| -> Seat {
            let (deck, sideboard) = pools::build_deck(db, &p.cards, &self.gen);
            Seat { deck, sideboard, first_picks: p.first_picks }
        };
        if self.pools {
            match &self.decks {
                DeckMode::Random => {
                    let mut rng = Rng::new(seed ^ 0x9001_5EED_0B03);
                    return pools::random_pools(db, &mut rng).map(built);
                }
                DeckMode::Eval => {
                    let i = (zero_search::rng::split64(seed) % self.eval_decks as u64) as u32;
                    return pools::eval_pools(db, self.eval_seed, i).map(built);
                }
                _ => {}
            }
        }
        let seat = |(deck, sideboard): (Vec<DefId>, Vec<DefId>)| Seat { deck, sideboard, first_picks: Vec::new() };
        let p: [(Vec<DefId>, Vec<DefId>); 2] = match &self.decks {
            DeckMode::Random => {
                let mut rng = Rng::new(seed ^ 0x00DE_C50F_2A11);
                let mut side = Rng::new(seed ^ 0x51DE_B0A2_D5EE);
                [decks::generate_pool(db, &mut rng, &mut side, &self.gen), decks::generate_pool(db, &mut rng, &mut side, &self.gen)]
            }
            DeckMode::Eval => {
                let i = (zero_search::rng::split64(seed) % self.eval_decks as u64) as u32;
                decks::eval_pools(db, &self.gen, self.eval_seed, i)
            }
            DeckMode::Lists(a, b) => [(a.clone(), Vec::new()), (b.clone(), Vec::new())],
            DeckMode::ListsWithSideboards(p) => p.clone(),
        };
        p.map(seat)
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

/// One match of Magic for zero-core: its current (or last) game and the
/// rest of the match (ENGINE.md 36).
#[derive(Clone)]
pub struct MtgAz {
    pub g: Game,
    pub m: MatchState,
}

impl MtgAz {
    pub fn new(spec: &MtgSpec, seed: u64) -> Result<MtgAz, String> {
        let seats = spec.seats(seed);
        // Best-of-three: the decks are hidden within the pools (ENGINE.md
        // 36.11); best-of-one keeps open decklists (M5a-M8a's game).
        let gspec = GameSpec {
            max_turns: spec.max_turns,
            mulligans: spec.mulligans,
            chooser: spec.chooser,
            hidden_decks: spec.format == Format::Bo3,
            ..GameSpec::default()
        };
        let [a, b] = &seats;
        let g = Game::with_sideboards(CardDb::cube(), gspec, [&a.deck, &b.deck], [&a.sideboard, &b.sideboard], seed)?;
        let m = MatchState::new(CardDb::cube(), spec.format, gspec, &seats, seed)?;
        let mut az = MtgAz { g, m };
        az.set_known_pools();
        Ok(az)
    }
}

/// The `heuristic` bot as a zero-core `Bot`.
struct HeuristicBot(bots::Heuristic);

impl Bot<MtgAz> for HeuristicBot {
    fn choose(&mut self, g: &MtgAz, legal: &[u16]) -> u16 {
        match g.sideboarding() {
            Some(sb) => bots::sideboard(sb, legal),
            None => self.0.choose(&g.g, legal),
        }
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
        match &self.m.sb {
            Some(sb) => Some(sb.decider() as usize),
            None => self.g.to_move().map(|p| p as usize),
        }
    }

    fn legal_actions(&self, out: &mut Vec<u16>) {
        match &self.m.sb {
            Some(sb) => {
                out.clear();
                out.extend(sb.decision.options.iter().map(|o| o.action));
            }
            None => self.g.legal_actions(out),
        }
    }

    fn step(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        self.match_step(a, chance)
    }

    fn force_chance(&mut self, forced: &[u64]) {
        self.g.force_chance(forced);
    }

    fn result(&self) -> [f32; MAX_SEATS] {
        let mut r = [0.0; MAX_SEATS];
        let v = match self.m.format {
            Format::Bo1 if self.g.is_over() => self.g.result_values(),
            Format::Bo3 if self.m.over => self.m.result(),
            _ => [0.5, 0.5],
        };
        r[0] = v[0];
        r[1] = v[1];
        r
    }

    /// Best-of-one: the game's life difference / 20. Best-of-three
    /// (ENGINE.md 36.6): 0.75 x the game-win difference / 2 + 0.25 x the
    /// current (at the end: the last) game's life margin.
    fn margin(&self, seat: usize) -> f32 {
        let me = self.g.life(seat as u8).max(0) as f32;
        let them = self.g.life(1 - seat as u8).max(0) as f32;
        let life = ((me - them) / 20.0).clamp(-1.0, 1.0);
        match self.m.format {
            Format::Bo1 => life,
            Format::Bo3 => {
                let w = (self.m.wins[seat] as f32 - self.m.wins[1 - seat] as f32) / 2.0;
                0.75 * w.clamp(-1.0, 1.0) + 0.25 * life
            }
        }
    }

    /// Life (best-of-one), game wins (best-of-three).
    fn score(&self, seat: usize) -> f32 {
        match self.m.format {
            Format::Bo1 => self.g.life(seat as u8) as f32,
            Format::Bo3 => self.m.wins[seat] as f32,
        }
    }

    /// The game's hidden information (not while sideboarding: the last
    /// game is over), and the match's RNG that seeds the later games.
    /// Best-of-three (ENGINE.md 36.11): first the opponent's unknown first
    /// picks, deck and sideboard (and between games their chosen deck),
    /// sampled from what the viewer knows (`sample.rs`).
    fn determinize(&mut self, viewer: usize, rng: &mut zero_search::rng::Rng) {
        let mut r = Rng::new(rng.next_u64());
        if self.m.format == Format::Bo3 && !self.m.over {
            self.sample_hidden_pool(viewer as u8, &mut r);
        }
        if self.m.sb.is_none() {
            self.g.determinize(viewer as u8, &mut r);
        }
        self.m.rng = Rng::new(r.next_u64());
    }

    fn encode(&self, viewer: usize, out: &mut [f32]) {
        if self.m.sb.is_some() {
            encode::encode_sideboarding(self, viewer as u8, out);
        } else {
            self.g.encode_features(viewer as u8, out);
            encode::encode_match(self, viewer as u8, out);
        }
    }

    /// The turns of the match's games.
    fn rounds(&self) -> u32 {
        self.m.turns_before + if self.m.format == Format::Bo3 && (self.m.over || self.m.sb.is_some()) { 0 } else { self.g.turn().number }
    }

    /// Minor: which mana source to tap next while paying (ENGINE.md 22; zero-core searches it
    /// with the seat's `sims_minor` and never records it).
    fn minor_decision(&self) -> bool {
        self.g
            .pending_decision()
            .is_some_and(|d| matches!(d.kind, crate::decision::DecisionKind::PayMana | crate::decision::DecisionKind::PayPool))
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
