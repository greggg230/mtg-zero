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
pub mod builder;
pub mod collation;

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
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Explore => "explore",
            Mode::Best => "best",
            Mode::Given => "given",
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

    /// Both seats of the game with seed `seed`.
    pub fn seats(&self, seed: u64) -> Result<[Seat; 2], String> {
        Ok([self.seat(seed, 0)?.0, self.seat(seed, 1)?.0])
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
