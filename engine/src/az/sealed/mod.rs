//! Sealed deck play (DESIGN.md 9, milestone S2; ENGINE.md 101): the Edge of Eternities Play
//! Booster collations (`collation.rs`, a bit-identical port of `mtgz.sealed.collation`), the
//! randomised deck builder (`builder.rs`) and the `mtg` spec's `"sealed"` key: each game
//! generates (or is given) a pool per player, builds both decks and plays best-of-one on the EoE
//! card table (`CardDb::eoe`), reproducibly from the game seed.
//!
//! ```json
//! {"sealed": {"set": "eoe", "collation": "arena", "boosters": 6,
//!             "builder": {"lands": [16, 18], "splash": 0.3, ...},
//!             "p0": {"pool": ["card", ...]},
//!             "p1": {"deck": ["card", ...], "sideboard": ["card", ...]}}}
//! ```
//!
//! `set`: `"eoe"` (the only set). `collation`: `"arena"` or `"paper"`, required when a seat's pool
//! is generated (as the reference, no default). `boosters`: per pool (6). `builder`: the builder's
//! settings (`builder::Settings`; every key optional). `p0` / `p1` (optional): a seat's given pool
//! (built by the builder every game, from the game seed) or a given deck and sideboard (played as
//! they are: a 17lands human build, `mtgz.sealed.yardstick`); a seat without one gets a generated
//! pool. Decklists stay open (best-of-one, `GameSpec::hidden_decks` false); ENGINE.md 101.5 lists
//! what hidden decklists need from here.
//!
//! A given deck may carry `"order": "given"` (ENGINE.md 104): its library before the setup shuffle
//! (CR 103.3) is the list in the order given, not sorted by definition (`"sorted"`, the default).
//! The shuffle is uniform either way, so a game's distribution is unchanged; what changes is which
//! card a position of the shuffled library holds for a given seed: two decks of one size laid out
//! slot by slot get the same permutation of their slots from the same seed (the paired deck
//! comparison's common random numbers, `mtgz.sealed.compare`). Without the key nothing changes.
//!
//! `best_share` p (ENGINE.md 107.1, the mixed field; default 0): per game and per built seat (a
//! generated or given pool), with probability p the deck is the builder's best guess
//! (`builder::best_guess`: every random choice at its most likely value, no randomness), else the
//! randomised build as before; the draw is a generator of its own from the game's (pools') seed and
//! the seat ([`best_seat`]), so the pools and the randomised builds are the same draws with or
//! without the key. The hidden decklists' sealed prior samples the same mixture (ENGINE.md 107.1).
//! Without the key (or at 0) nothing changes.
//!
//! `deck_choice` (ENGINE.md 107.2): `{"k": 32, "explore": 0.1}` - each built seat (a generated or
//! given pool) has K candidate builds of its pool ([`SealedSpec::candidates`]: candidate 0 the best
//! guess, 1..K the randomised builder from generators of their own); with probability `explore` (a
//! generator of its own, [`explore_pick`]) it plays candidate 1 + a uniform of the K - 1 randomised
//! ones, else it **chooses** among the K in a pre-game decision (`DecisionKind::ChooseDeck`, the
//! match's: `MtgAz::new`, `matches.rs`), hidden from the opponent. Needs hidden decklists. Without
//! the key nothing changes.
//!
//! The deck choice **by an evaluator** (ENGINE.md 112): `{"k": 32, "explore": 0.1, "by": "evaluator", "evaluator":
//! "<path>.json", "evaluator_sha": "<sha256>", "temperature": 0}` - a choosing seat scores its own pool's K
//! candidates with the exported deck evaluator ([`scorer`], read from the file; `evaluator_sha` optional, a file
//! with other contents an error) and plays the best (the first on a tie), or with `temperature` t > 0 one drawn with
//! probability proportional to exp((score - best) / t) from a generator of its own ([`choice_rng`]). No decision:
//! the deck is chosen when the game is made, from the seat's own pool alone. `"seats": [0]` (either deck choice):
//! only these built seats choose (or explore); the others play the spec's seat (`best_share`'s mixed field).
//! Without `by` (or `"by": "value"`) the value choice of before.
pub mod builder;
pub mod collation;
pub mod scorer;

use std::sync::Arc;

use serde_json::{json, Value};

use crate::db::{types, CardDb};
use crate::ids::DefId;
use crate::rng::{split64, Rng};

use super::constructed::MAX_DECK;
use super::decks::DECK_SIZE;
use super::matches::Seat;
use builder::{Build, Settings};
use collation::{Collation, Options};

/// Where a seat's deck comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeatSource {
    /// A pool generated from the game seed (the spec's collation), built by the builder.
    Generated,
    /// A given pool (nonbasic cards, sorted), built by the builder every game.
    Pool(Vec<DefId>),
    /// A given deck and sideboard (sorted), played as they are; `ordered` (`"order": "given"`): the
    /// deck in its given order instead, the library's order before the setup shuffle (module docs).
    Build { deck: Vec<DefId>, sideboard: Vec<DefId>, ordered: bool },
}

/// The `mtg` spec's `"sealed"` object (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedSpec {
    pub collation: Option<Collation>,
    pub boosters: usize,
    pub builder: Settings,
    pub seats: [SeatSource; 2],
    /// The mixed field (ENGINE.md 107.1, `"best_share"`, default 0): per game and seat (a built seat:
    /// a generated or given pool), the deck is the builder's best guess (`builder::best_guess`) with
    /// this probability, else the randomised build; drawn from the game's seed ([`best_seat`]).
    pub best_share: super::Share,
    /// The deck choice (ENGINE.md 107.2, `"deck_choice"`; None: none).
    pub deck_choice: Option<DeckChoiceSpec>,
}

/// The sealed spec's `"deck_choice"` (ENGINE.md 107.2): K candidate builds per built seat and the
/// share of seats that play a random one instead of choosing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckChoiceSpec {
    /// Candidates per seat, 2..=32 (`"k"`, default 32; the choice's actions are `NUMBER` + j).
    pub k: u8,
    /// The share of built seats that play a uniformly drawn randomised candidate (1..K) without a
    /// choice (`"explore"`, default 0.1; the field's builder-random decks).
    pub explore: super::Share,
    /// Who chooses (`"by"`; ENGINE.md 112): the network's value in a pre-game decision (107.2), or an evaluator.
    pub by: ChoiceBy,
    /// The built seats that choose (`"seats"`, default both): another seat plays the spec's seat.
    pub seats: [bool; 2],
}

/// Who makes a seat's deck choice (ENGINE.md 112).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceBy {
    /// The network's value of the game started with each candidate: the pre-game decision (ENGINE.md 107.2).
    Value,
    /// The exported deck evaluator, when the game is made (module docs).
    Evaluator(EvalChoice),
}

/// The deck choice by an evaluator: the scorer and the temperature.
#[derive(Clone, Debug)]
pub struct EvalChoice {
    pub scorer: Arc<scorer::Scorer>,
    /// 0: the best candidate; t > 0: one drawn with probability proportional to exp((score - best) / t).
    pub temperature: f64,
}

// (The spec is `Eq`: an evaluator by its file and contents, the temperature by its bits.)
impl PartialEq for EvalChoice {
    fn eq(&self, o: &Self) -> bool {
        self.scorer.path == o.scorer.path && self.scorer.sha256 == o.scorer.sha256 && self.temperature.to_bits() == o.temperature.to_bits()
    }
}
impl Eq for EvalChoice {}

impl EvalChoice {
    /// The choice among `decks` (seat `p` of the game whose pools come from `seed`): (the candidate, every
    /// candidate's score). Argmax (the first on a tie) at temperature 0, else a softmax draw from [`choice_rng`].
    pub fn choose(&self, decks: &[&[DefId]], seed: u64, p: u8) -> Result<(u8, Vec<f64>), String> {
        let scores = decks.iter().map(|d| self.scorer.score(d)).collect::<Result<Vec<f64>, String>>()?;
        let j = pick_by_score(&scores, self.temperature, &mut choice_rng(seed, p));
        Ok((j as u8, scores))
    }
}

/// The best of `scores` (the first on a tie) at temperature 0 (no draw), else index i drawn with probability
/// proportional to exp((s_i - best) / t) by `rng` (one draw).
pub fn pick_by_score(scores: &[f64], t: f64, rng: &mut Rng) -> usize {
    let best = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if t <= 0.0 {
        return scores.iter().position(|&s| s == best).expect("a candidate");
    }
    let w: Vec<f64> = scores.iter().map(|&s| ((s - best) / t).exp()).collect();
    let total: f64 = w.iter().sum();
    let mut u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * total;
    for (i, x) in w.iter().enumerate() {
        if u < *x {
            return i;
        }
        u -= x;
    }
    w.len() - 1
}

/// The generator of seat `p`'s evaluator choice at a temperature (ENGINE.md 112) in the game whose pools come from
/// `seed`: its own, so the pools, the candidates and the explore draw are the same with or without it.
pub fn choice_rng(seed: u64, p: u8) -> Rng {
    Rng::new(split64(seed ^ 0x0C40_05E5_0000 ^ ((p as u64) << 40)))
}

/// The largest K of a deck choice (`act::NUMBER_N`: its actions are `NUMBER` + j).
pub const MAX_CANDIDATES: u8 = crate::decision::act::NUMBER_N as u8;

/// Whether seat `p` of the game whose pools come from `seed` plays a random candidate instead of
/// choosing (ENGINE.md 107.2): Some(j), j in 1..k (a uniform of the randomised candidates), with
/// probability `explore`, from a generator of its own; else None (it chooses). No draw at 0.
pub fn explore_pick(seed: u64, p: u8, explore: f64, k: u8) -> Option<u8> {
    if explore <= 0.0 {
        return None;
    }
    let mut r = Rng::new(split64(seed ^ 0xE7B1_0EE5_0000 ^ ((p as u64) << 40)));
    let u = (r.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
    (u < explore).then(|| 1 + r.below(k as u32 - 1) as u8)
}

/// Candidate j's builder generator (j >= 1) of seat `p` in the game whose pools come from `seed`.
pub fn candidate_rng(seed: u64, p: u8, j: u8) -> Rng {
    Rng::new(split64(seed ^ 0xDEC5_C401_CE00 ^ ((p as u64) << 40) ^ ((j as u64) << 48)))
}

/// How a seat's deck was built (ENGINE.md 107.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The randomised builder (`builder::build`): the exploratory builds.
    Explore,
    /// The builder's best guess (`builder::best_guess`).
    Best,
    /// A given deck, played as given.
    Given,
    /// Chosen among the candidates by the deck choice (ENGINE.md 107.2).
    Chosen,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Explore => "explore",
            Mode::Best => "best",
            Mode::Given => "given",
            Mode::Chosen => "chosen",
        }
    }
}

/// How a seat's deck was made in one game (for reports and the Python helpers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeatInfo {
    /// The pool's seed (a generated pool), else None.
    pub pool_seed: Option<u64>,
    /// The pool (nonbasic cards, sorted).
    pub pool: Vec<DefId>,
    /// The builder's build (a generated or given pool), else None (a given deck).
    pub build: Option<Build>,
    /// Which builder made the deck.
    pub mode: Mode,
}

/// Does seat `p` of the game whose pools and builds come from `seed` play the best guess, with the
/// mixed field's share `share` (ENGINE.md 107.1)? A uniform draw of a generator of its own (seeded by
/// `seed` and `p`, never the pool's or the build's), below `share`; no draw at 0.
pub fn best_seat(seed: u64, p: u8, share: f64) -> bool {
    share > 0.0 && {
        let mut r = Rng::new(split64(seed ^ 0xBE57_6E55_0000 ^ ((p as u64) << 40)));
        ((r.next_u64() >> 11) as f64 / (1u64 << 53) as f64) < share
    }
}

/// The card table of sealed play.
pub fn db() -> &'static CardDb {
    CardDb::eoe()
}

/// A card name of the EoE table as a card definition: an unknown name or an unsupported card fails
/// loudly (with the blockers), as does a token.
pub fn card_id(db: &CardDb, name: &str) -> Result<DefId, String> {
    let id = db.expect_id(name)?;
    if db.def(id).token {
        return Err(format!("{name} is a token"));
    }
    Ok(id)
}

/// Card names as definitions (sorted), every one mapped or an error naming the first that isn't.
pub fn card_ids<S: AsRef<str>>(db: &CardDb, names: &[S]) -> Result<Vec<DefId>, String> {
    let mut v = names.iter().map(|n| card_id(db, n.as_ref())).collect::<Result<Vec<DefId>, String>>()?;
    v.sort_unstable();
    Ok(v)
}

/// Seat `p`'s pool seed in a game with seed `seed`.
pub fn pool_seed(seed: u64, p: u8) -> u64 {
    split64(seed ^ 0x5EA1_ED00_9001 ^ ((p as u64) << 40))
}

/// Seat `p`'s builder generator in a game with seed `seed`.
pub fn build_rng(seed: u64, p: u8) -> Rng {
    Rng::new(split64(seed ^ 0xB011_DDEC_0000 ^ ((p as u64) << 40)))
}

/// The seed that stands for evaluation pair `i` of the evaluation set `eval_seed` (its pools and
/// builds are a game's with this seed).
pub fn eval_game_seed(eval_seed: u64, i: u32) -> u64 {
    split64(eval_seed.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ (0x5EA1_E7A1_0000 + i as u64))
}

/// The nonbasic cards of a generated pool as definitions, or the first that has none.
pub fn generated_pool(db: &CardDb, collation: Collation, boosters: usize, seed: u64) -> Result<Vec<DefId>, String> {
    let p = collation::generate(seed, collation, Options { boosters, ..Options::default() })?;
    card_ids(db, &p.cards()).map_err(|e| format!("sealed pool ({} seed {seed}): {e}", collation.name()))
}

fn names(db: &CardDb, ids: &[DefId]) -> Value {
    Value::Array(ids.iter().map(|&id| Value::String(db.def(id).name.clone())).collect())
}

impl SealedSpec {
    /// Parse the spec's `"sealed"` object (module docs).
    pub fn parse(v: &Value) -> Result<SealedSpec, String> {
        let db = db();
        let o = v.as_object().ok_or("mtg spec: sealed must be a JSON object")?;
        let mut s = SealedSpec {
            collation: None,
            boosters: collation::BOOSTERS,
            builder: Settings::default(),
            seats: [SeatSource::Generated, SeatSource::Generated],
            best_share: super::Share(0.0),
            deck_choice: None,
        };
        let mut set = None;
        for (k, x) in o {
            match k.as_str() {
                "set" => set = Some(x.as_str().ok_or("mtg spec: sealed.set must be a string")?),
                "collation" => s.collation = Some(Collation::parse(x.as_str().unwrap_or("")).map_err(|e| format!("mtg spec: sealed.{e}"))?),
                "boosters" => {
                    let n = x.as_u64().filter(|n| (1..=12).contains(n)).ok_or("mtg spec: sealed.boosters must be 1..=12")?;
                    s.boosters = n as usize;
                }
                "builder" => s.builder = Settings::parse(x).map_err(|e| format!("mtg spec: {e}"))?,
                "best_share" => {
                    let p = x.as_f64().filter(|p| (0.0..=1.0).contains(p)).ok_or("mtg spec: sealed.best_share must be a probability (0..=1)")?;
                    s.best_share = super::Share(p);
                }
                "deck_choice" => s.deck_choice = Some(DeckChoiceSpec::parse(x)?),
                "p0" | "p1" => {
                    let p = (k == "p1") as usize;
                    s.seats[p] = Self::parse_seat(db, k, x)?;
                }
                other => return Err(format!("mtg spec: unknown sealed key {other:?}")),
            }
        }
        match set {
            Some("eoe") => {}
            Some(x) => return Err(format!("mtg spec: sealed.set {x:?}: the only set is \"eoe\"")),
            None => return Err("mtg spec: sealed.set is required (\"eoe\")".into()),
        }
        if s.collation.is_none() && s.seats.iter().any(|x| *x == SeatSource::Generated) {
            return Err("mtg spec: sealed.collation (\"arena\" or \"paper\") is required for a generated pool".into());
        }
        Ok(s)
    }

    fn parse_seat(db: &CardDb, k: &str, x: &Value) -> Result<SeatSource, String> {
        let o = x.as_object().ok_or_else(|| format!("mtg spec: sealed.{k} must be an object"))?;
        // `sorted`: by definition (every list but an ordered deck), else in the given order
        let ids = |key: &str, sorted: bool| -> Result<Vec<DefId>, String> {
            let a = o.get(key).and_then(|v| v.as_array()).ok_or_else(|| format!("mtg spec: sealed.{k}.{key} must be a list of card names"))?;
            let names: Vec<&str> = a.iter().map(|n| n.as_str().ok_or("mtg spec: card names are strings")).collect::<Result<_, _>>()?;
            let v = if sorted { card_ids(db, &names) } else { names.iter().map(|n| card_id(db, n)).collect() };
            v.map_err(|e| format!("mtg spec: sealed.{k}.{key}: {e}"))
        };
        let list = |key: &str| ids(key, true);
        for key in o.keys() {
            if !["pool", "deck", "sideboard", "order"].contains(&key.as_str()) {
                return Err(format!("mtg spec: unknown sealed.{k} key {key:?}"));
            }
        }
        // ENGINE.md 104: a given deck's order (module docs)
        let ordered = match o.get("order") {
            None => false,
            Some(x) => match x.as_str() {
                Some("given") => true,
                Some("sorted") => false,
                _ => return Err(format!("mtg spec: sealed.{k}.order must be \"given\" or \"sorted\", not {x}")),
            },
        };
        let basics = super::pools::basic_ids(db);
        if o.contains_key("pool") {
            if o.contains_key("deck") || o.contains_key("sideboard") {
                return Err(format!("mtg spec: sealed.{k}: a pool or a deck and sideboard, not both"));
            }
            if o.contains_key("order") {
                return Err(format!("mtg spec: sealed.{k}.order is a given deck's (a pool's deck is the builder's)"));
            }
            let pool = list("pool")?;
            if let Some(&b) = pool.iter().find(|d| basics.contains(d)) {
                return Err(format!("mtg spec: sealed.{k}.pool holds a basic land ({}): basics are free, not pool cards", db.def(b).name));
            }
            return Ok(SeatSource::Pool(pool));
        }
        let deck = ids("deck", !ordered)?;
        let sideboard = if o.contains_key("sideboard") { list("sideboard")? } else { Vec::new() };
        if let Some(&b) = sideboard.iter().find(|d| basics.contains(d)) {
            return Err(format!("mtg spec: sealed.{k}.sideboard holds a basic land ({}): basics are free, not pool cards", db.def(b).name));
        }
        // CR 100.2b: a limited deck has at least 40 cards (a given build is played as it was
        // built; the builder's decks are exactly 40).
        if deck.len() < DECK_SIZE || deck.len() > MAX_DECK {
            return Err(format!("mtg spec: sealed.{k}.deck has {} cards (40..={MAX_DECK})", deck.len()));
        }
        Ok(SeatSource::Build { deck, sideboard, ordered })
    }

    /// The spec as JSON (what `parse` reads back to an equal spec; given cards by name).
    pub fn to_json(&self) -> Value {
        let db = db();
        let mut o = json!({"set": "eoe", "boosters": self.boosters, "builder": self.builder.to_json()});
        if let Some(c) = self.collation {
            o["collation"] = json!(c.name());
        }
        if self.best_share.0 > 0.0 {
            o["best_share"] = json!(self.best_share.0);
        }
        if let Some(c) = &self.deck_choice {
            o["deck_choice"] = c.to_json();
        }
        for (p, s) in self.seats.iter().enumerate() {
            let v = match s {
                SeatSource::Generated => continue,
                SeatSource::Pool(pool) => json!({"pool": names(db, pool)}),
                SeatSource::Build { deck, sideboard, ordered: false } => json!({"deck": names(db, deck), "sideboard": names(db, sideboard)}),
                SeatSource::Build { deck, sideboard, ordered: true } => {
                    json!({"deck": names(db, deck), "sideboard": names(db, sideboard), "order": "given"})
                }
            };
            o[format!("p{p}")] = v;
        }
        o
    }

    /// Seat `p`'s deck in the game with seed `seed` (module docs), and how it was made.
    pub fn seat(&self, seed: u64, p: u8) -> Result<(Seat, SeatInfo), String> {
        let db = db();
        let (pool, pool_seed) = match &self.seats[p as usize] {
            SeatSource::Build { deck, sideboard, .. } => {
                let basics = super::pools::basic_ids(db);
                let mut pool: Vec<DefId> = deck.iter().copied().filter(|d| !basics.contains(d)).chain(sideboard.iter().copied()).collect();
                pool.sort_unstable();
                let seat = Seat { deck: deck.clone(), sideboard: sideboard.clone(), first_picks: Vec::new(), elsewhere: Vec::new() };
                return Ok((seat, SeatInfo { pool_seed: None, pool, build: None, mode: Mode::Given }));
            }
            SeatSource::Pool(pool) => (pool.clone(), None),
            SeatSource::Generated => {
                let ps = pool_seed(seed, p);
                let c = self.collation.expect("parse: a collation for a generated pool");
                (generated_pool(db, c, self.boosters, ps).map_err(|e| format!("player {p}: {e}"))?, Some(ps))
            }
        };
        // ENGINE.md 107.1: the mixed field's best guess, or the randomised build (its generator untouched)
        let (b, mode) = if best_seat(seed, p, self.best_share.0) {
            (builder::best_guess(db, &pool, &self.builder), Mode::Best)
        } else {
            (builder::build(db, &pool, &self.builder, &mut build_rng(seed, p)), Mode::Explore)
        };
        let seat = Seat { deck: b.deck.clone(), sideboard: b.sideboard.clone(), first_picks: Vec::new(), elsewhere: Vec::new() };
        Ok((seat, SeatInfo { pool_seed, pool, build: Some(b), mode }))
    }

    /// Seat `p`'s pool in the game whose pools come from `seed` and its seed (a generated one), or
    /// None for a given deck.
    pub fn pool_of(&self, seed: u64, p: u8) -> Result<Option<(Vec<DefId>, Option<u64>)>, String> {
        Ok(match &self.seats[p as usize] {
            SeatSource::Build { .. } => None,
            SeatSource::Pool(pool) => Some((pool.clone(), None)),
            SeatSource::Generated => {
                let ps = pool_seed(seed, p);
                let c = self.collation.expect("parse: a collation for a generated pool");
                Some((generated_pool(db(), c, self.boosters, ps).map_err(|e| format!("player {p}: {e}"))?, Some(ps)))
            }
        })
    }

    /// Seat `p`'s K candidate builds (ENGINE.md 107.2) in the game whose pools come from `seed`: 0 the
    /// builder's best guess, j >= 1 the randomised builder from `candidate_rng(seed, p, j)` (copies
    /// possible); None for a given deck or without a deck choice. With the pool's seed.
    pub fn candidates(&self, seed: u64, p: u8) -> Result<Option<(Vec<Build>, Option<u64>)>, String> {
        let Some(c) = self.deck_choice.as_ref().filter(|c| c.seats[p as usize]) else { return Ok(None) };
        let Some((pool, ps)) = self.pool_of(seed, p)? else { return Ok(None) };
        let db = db();
        let mut v = Vec::with_capacity(c.k as usize);
        v.push(builder::best_guess(db, &pool, &self.builder));
        for j in 1..c.k {
            v.push(builder::build(db, &pool, &self.builder, &mut candidate_rng(seed, p, j)));
        }
        Ok(Some((v, ps)))
    }

    /// Both seats of the game with seed `seed`.
    pub fn seats(&self, seed: u64) -> Result<[Seat; 2], String> {
        Ok([self.seat(seed, 0)?.0, self.seat(seed, 1)?.0])
    }
}

impl DeckChoiceSpec {
    /// The spec's `"deck_choice"` object: `k` (2..=32, default 32), `explore` (a probability, default
    /// 0.1), `by` (`"value"`, the default, or `"evaluator"` with `evaluator` a file of the exported evaluator,
    /// `evaluator_sha` its sha256 if pinned and `temperature` >= 0, default 0), `seats` (the choosing seats, default
    /// both); an unknown key is an error, as is a key of the evaluator's without it.
    pub fn parse(v: &Value) -> Result<DeckChoiceSpec, String> {
        let o = v.as_object().ok_or("mtg spec: sealed.deck_choice must be an object {\"k\": 32, \"explore\": 0.1}")?;
        let mut c = DeckChoiceSpec { k: MAX_CANDIDATES, explore: super::Share(0.1), by: ChoiceBy::Value, seats: [true, true] };
        let mut by = None;
        let (mut path, mut sha, mut temperature) = (None, None, None);
        for (k, x) in o {
            match k.as_str() {
                "k" => {
                    c.k = x
                        .as_u64()
                        .filter(|n| (2..=MAX_CANDIDATES as u64).contains(n))
                        .ok_or_else(|| format!("mtg spec: sealed.deck_choice.k must be 2..={MAX_CANDIDATES}"))? as u8
                }
                "explore" => {
                    let p = x.as_f64().filter(|p| (0.0..=1.0).contains(p)).ok_or("mtg spec: sealed.deck_choice.explore must be a probability (0..=1)")?;
                    c.explore = super::Share(p);
                }
                "by" => by = Some(x.as_str().filter(|b| ["value", "evaluator"].contains(b)).ok_or("mtg spec: sealed.deck_choice.by must be \"value\" or \"evaluator\"")?),
                "evaluator" => path = Some(x.as_str().filter(|p| !p.is_empty()).ok_or("mtg spec: sealed.deck_choice.evaluator must be a file's path")?),
                "evaluator_sha" => {
                    let h = x.as_str().and_then(scorer::parse_hex32).ok_or("mtg spec: sealed.deck_choice.evaluator_sha must be a sha256 (64 hex digits)")?;
                    sha = Some(h);
                }
                "temperature" => {
                    temperature = Some(x.as_f64().filter(|t| t.is_finite() && *t >= 0.0).ok_or("mtg spec: sealed.deck_choice.temperature must be a number >= 0")?)
                }
                "seats" => {
                    let a = x.as_array().filter(|a| !a.is_empty()).ok_or("mtg spec: sealed.deck_choice.seats must be a list of seats (0, 1)")?;
                    c.seats = [false, false];
                    for s in a {
                        match s.as_u64() {
                            Some(p @ (0 | 1)) if !c.seats[p as usize] => c.seats[p as usize] = true,
                            _ => return Err(format!("mtg spec: sealed.deck_choice.seats {x}: seats 0 and 1, each once")),
                        }
                    }
                }
                other => return Err(format!("mtg spec: unknown sealed.deck_choice key {other:?} (k, explore, by, evaluator, evaluator_sha, temperature, seats)")),
            }
        }
        if by == Some("evaluator") {
            let path = path.ok_or("mtg spec: sealed.deck_choice by \"evaluator\" needs \"evaluator\": the exported evaluator's file (ENGINE.md 112)")?;
            let s = scorer::load(path, db()).map_err(|e| format!("mtg spec: sealed.deck_choice: {e}"))?;
            if let Some(h) = sha.filter(|h| *h != s.sha256) {
                return Err(format!("mtg spec: sealed.deck_choice: {path} has sha256 {}, not the spec's {}", s.sha_hex(), scorer::hex(&h)));
            }
            // every card a pool can hold, and the basics, must be in its table (a candidate is never unscorable)
            use super::hidden_decks::DeckPrior;
            let support = super::hidden_decks::SealedPrior::new(Collation::Arena, collation::BOOSTERS, Settings::default());
            if let Some(&d) = support.support().iter().find(|&&d| !s.has(d)) {
                return Err(format!("mtg spec: sealed.deck_choice: {path}: its card table lacks {} (a card of the set's pools)", db().def(d).name));
            }
            c.by = ChoiceBy::Evaluator(EvalChoice { scorer: s, temperature: temperature.unwrap_or(0.0) });
        } else if path.is_some() || sha.is_some() || temperature.is_some() {
            return Err("mtg spec: sealed.deck_choice: evaluator, evaluator_sha and temperature are the deck choice by an evaluator's (\"by\": \"evaluator\")".into());
        }
        Ok(c)
    }

    /// The object as JSON (what `parse` reads back to an equal spec): the value choice's keys as before (`seats`
    /// only when not both); by an evaluator its file, its sha256 and the temperature.
    pub fn to_json(&self) -> Value {
        let mut o = json!({"k": self.k, "explore": self.explore.0});
        if let ChoiceBy::Evaluator(e) = &self.by {
            o["by"] = json!("evaluator");
            o["evaluator"] = json!(e.scorer.path);
            o["evaluator_sha"] = json!(e.scorer.sha_hex());
            o["temperature"] = json!(e.temperature);
        }
        if self.seats != [true, true] {
            o["seats"] = json!((0..2).filter(|&p| self.seats[p]).collect::<Vec<_>>());
        }
        o
    }

    /// The deck choice by an evaluator, if this is one.
    pub fn evaluator(&self) -> Option<&EvalChoice> {
        match &self.by {
            ChoiceBy::Evaluator(e) => Some(e),
            ChoiceBy::Value => None,
        }
    }
}

/// A build as JSON (names; colours as letters).
pub fn build_json(db: &CardDb, b: &Build) -> Value {
    json!({
        "deck": names(db, &b.deck), "sideboard": names(db, &b.sideboard),
        "colors": builder::letters(b.colors), "splash": builder::letters(b.splash),
        "lands": b.lands, "splash_spells": b.splash_spells, "tilt": b.tilt_milli as f64 / 1000.0,
        "spells": b.deck.iter().filter(|&&id| !db.def(id).is(types::LAND)).count(),
    })
}

/// Seat `p` of a game as JSON (`sealed_seats` in Python).
pub fn seat_json(db: &CardDb, seat: &Seat, info: &SeatInfo) -> Value {
    let mut v = json!({
        "deck": names(db, &seat.deck), "sideboard": names(db, &seat.sideboard), "pool": names(db, &info.pool),
        "pool_seed": info.pool_seed, "mode": info.mode.name(),
    });
    if let Some(b) = &info.build {
        v["build"] = build_json(db, b);
    }
    v
}

/// A generated pool as JSON: each booster's [slot, name] pairs, the promo, the nonbasic cards
/// (the Python binding's `sealed_pool`; the reference comparison, `tests/test_sealed_rust.py`).
pub fn pool_json(seed: u64, collation: &str, boosters: usize, prerelease_promo: bool, paradox_haze: bool) -> Result<String, String> {
    let c = Collation::parse(collation)?;
    let p = collation::generate(seed, c, Options { boosters, prerelease_promo, paradox_haze })?;
    let b: Vec<Vec<[&str; 2]>> = p.boosters.iter().map(|b| b.iter().map(|&(s, n)| [s.name(), n]).collect()).collect();
    Ok(json!({"seed": seed, "collation": c.name(), "boosters": b, "promo": p.promo, "cards": p.cards()}).to_string())
}

/// The builder on a pool of names with a seed and settings (JSON object or empty), as JSON (the
/// Python binding's `sealed_build`).
pub fn build_names_json(pool: &[String], seed: u64, settings: &str) -> Result<String, String> {
    let db = db();
    let s = if settings.trim().is_empty() { Settings::default() } else {
        Settings::parse(&serde_json::from_str::<Value>(settings).map_err(|e| format!("sealed builder: {e}"))?)?
    };
    let ids = card_ids(db, pool)?;
    let b = builder::build(db, &ids, &s, &mut Rng::new(seed));
    Ok(build_json(db, &b).to_string())
}

/// A deck-choice spec's candidates of a game (ENGINE.md 107.2) as JSON (the Python binding's
/// `sealed_candidates`): per seat `{"pool_seed", "explore": j or null, "candidates": [build JSON, ...]}`
/// (null for a given deck).
pub fn candidates_json(spec: &str, seed: u64) -> Result<String, String> {
    let sp = super::MtgSpec::parse(spec)?;
    let s = sp.sealed.as_deref().ok_or("sealed_candidates: not a sealed spec")?;
    let c = s.deck_choice.as_ref().ok_or("sealed_candidates: not a deck-choice spec (sealed.deck_choice)")?;
    let seed = sp.sealed_seed(seed);
    let db = db();
    let mut out = Vec::new();
    for p in 0..2u8 {
        out.push(match s.candidates(seed, p)? {
            None => Value::Null,
            Some((v, ps)) => {
                let explore = explore_pick(seed, p, c.explore.0, c.k);
                let mut x = json!({
                    "pool_seed": ps, "explore": explore,
                    "candidates": v.iter().map(|b| build_json(db, b)).collect::<Vec<_>>(),
                });
                // (ENGINE.md 112: the evaluator's scores of every candidate, and its choice unless the seat explores)
                if let Some(e) = c.evaluator() {
                    let decks: Vec<&[DefId]> = v.iter().map(|b| &b.deck[..]).collect();
                    let (j, scores) = e.choose(&decks, seed, p)?;
                    x["scores"] = json!(scores);
                    x["chosen"] = if explore.is_none() { json!(j) } else { Value::Null };
                    x["evaluator_sha"] = json!(e.scorer.sha_hex());
                }
                x
            }
        });
    }
    Ok(Value::Array(out).to_string())
}

/// The scores of decks (a JSON list of lists of card names) by the exported evaluator at `path` (ENGINE.md 112;
/// the Python binding's `sealed_scores`, the check against the torch model), as JSON.
pub fn scores_json(path: &str, decks: &str) -> Result<String, String> {
    let db = db();
    let s = scorer::load(path, db)?;
    let decks: Vec<Vec<String>> = serde_json::from_str(decks).map_err(|e| format!("sealed_scores: decks: {e}"))?;
    let mut out = Vec::with_capacity(decks.len());
    for d in &decks {
        let ids = d.iter().map(|n| db.expect_id(n)).collect::<Result<Vec<DefId>, String>>()?;
        out.push(s.score(&ids)?);
    }
    Ok(json!(out).to_string())
}

/// What the engine reads of the exported evaluator at `path` (the binding's `sealed_scorer_info`), as JSON.
pub fn scorer_info_json(path: &str) -> Result<String, String> {
    let s = scorer::load(path, db())?;
    let model = match s.kind {
        scorer::Kind::Linear => json!({"kind": "linear"}),
        scorer::Kind::Set { d, k, hidden } => json!({"kind": "set", "d": d, "k": k, "hidden": hidden}),
    };
    Ok(json!({"path": s.path, "sha256": s.sha_hex(), "kind": model["kind"], "model": model, "members": s.members(), "cards": s.cards()}).to_string())
}

/// The seats of an `mtg` spec's game (a sealed spec) as JSON (the Python binding's
/// `sealed_seats`).
pub fn seats_json(spec: &str, seed: u64) -> Result<String, String> {
    let sp = super::MtgSpec::parse(spec)?;
    let s = sp.sealed.as_deref().ok_or("sealed_seats: not a sealed spec")?;
    let seed = sp.sealed_seed(seed);
    let db = db();
    let mut out = Vec::new();
    for p in 0..2 {
        let (seat, info) = s.seat(seed, p)?;
        out.push(seat_json(db, &seat, &info));
    }
    Ok(Value::Array(out).to_string())
}

/// Each name's definition in the EoE table, or why it has none (`{"name": id | "error"}`; the
/// 17lands yardstick's mapping, `mtgz.sealed.yardstick`).
pub fn lookup_json(names: &[String]) -> String {
    let db = db();
    let m: serde_json::Map<String, Value> =
        names.iter().map(|n| (n.clone(), match card_id(db, n) { Ok(id) => json!(id), Err(e) => json!(e) })).collect();
    Value::Object(m).to_string()
}

/// A shared spec (cheap clones of `MtgSpec`).
pub type Shared = Arc<SealedSpec>;
