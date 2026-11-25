//! A metagame spec (ENGINE.md 176): one constructed `mtg` spec whose games play every pairing of a list of
//! named 75s - each game (a best-of-three: each match) its own ordered pair (p0's deck, p1's deck), drawn from
//! that game's seed, so self-play, the Mac, the farm's release worker, the replays and the trainer's replay path
//! all play the same pairing for the same seed (each of them makes the game with `MtgAz::new_game(spec, seed)`).
//!
//! ```json
//! {"constructed": true, "format": "bo1", "max_turns": 60,
//!  "metagame": {"decks": [{"name": "izzet_delver_8016541", "main": [...], "sideboard": [...]},
//!                         {"name": "doomsday_8027957", "main": [...]},
//!                         {"name": "sneak_show_8014220", "main": [...]}],
//!               "weights": [[1, 1, 1], [1, 1, 1], [1, 1, 1]]}}
//! ```
//!
//! * `decks`: 1 to [`MAX_DECKS`] entries, each `{"name", "main", "sideboard"?}` (card names; `name` unique and
//!   non-empty, the game log's and the game record's name of the deck; no other key). Each is a constructed deck
//!   (CR 100.2a, 100.4a: `constructed::check`), in best-of-three too a main of any legal size N >= 60 (ENGINE.md 120, 272).
//! * `weights` (optional): an n x n matrix of non-negative integers, row = p0's deck, column = p1's deck (the
//!   ordered pair), at least one positive, their sum at most [`MAX_TOTAL`]. Default: every entry 1 - uniform over
//!   the n x n ordered pairs, mirrors included, so every cell gets the same share of games (and of training rows,
//!   up to the games' lengths). A weight 0 never plays that cell.
//! * The pairing of a game with seed `s`: index k of the cells (row-major) drawn with probability `w_k / sum w`
//!   EXACTLY (an integer below the total by rejection: no modulo bias) from `Rng::new(split64(s ^ SALT))`, a stream
//!   of its own - so everything else of the game (its shuffles, the play/draw, the match's later seeds) is exactly
//!   the game of the same seed whose spec names that pair's two 75s as `decks` lists (`pair_json`): a metagame game
//!   IS that fixed-pair game (`engine/tests/metagame.rs` checks it decision by decision).
//! * With `"constructed": true` only, instead of `decks`; best-of-one or constructed best-of-three
//!   (`game_episodes`, `start_game` 2 when every pair of positive weight is a mirror: game 1's result is drawn as a
//!   fair coin, ENGINE.md 130, 147.4). Refused (loudly: no silent no-op): `decks`, `seed` (one match seed would play
//!   one pairing), `start`, `build`, `sealed`, `pools`, `hidden_decklists`, `sideboard_table` (one 75's table: a
//!   metagame deck carries its own, below), `keep_deck` / `keep_known` - settings built for one pair of 75s.
//! * **Per-deck sideboarding settings** (ENGINE.md 240, best-of-three): `"dealt_decks"` (ENGINE.md 154, 168.1) deals
//!   each seat's 60s from ITS OWN 75 (every deck of a played pairing has a sideboard of at least the range's top), and
//!   a deck entry's optional `"table"` (a choice table of ENGINE.md 168.3, its list that deck's 75) is played by
//!   whichever seat plays that deck - the table is keyed by the deck, so it follows the deck to its seat.
//!   `pair_json` passes both through (`dealt_decks` as it is, a deck's table as its seat's `decks.pN_table`), so a
//!   metagame game is still exactly its pairing's fixed game.
//!
//! Nothing of a game reads the spec after `MtgAz::new` made it from the pairing's seats: the open decklists' pools,
//! the constructed prior of the hidden 60s, the deck target, the worlds `determinize` deals - all are the match's
//! own (`MatchState`), so they are this game's pairing's. `MatchState::pairing` names the pair (the game log's
//! lines, `record_setup`, the spectator view); it is a setting of the match from its spec and seed, the same in every
//! world, and never encoded (both 75s are public: the encoding shows the decks themselves).
use std::sync::Arc;

use serde_json::Value;

use super::matches::MatchPool;
use super::sbtable::SideboardTable;
use crate::db::CardDb;
use crate::ids::DefId;
use crate::rng::{split64, Rng};

/// At most this many decks (a pairing's indices are `u8`; 256 cells).
pub const MAX_DECKS: usize = 16;
/// The weights' largest sum (the draw is exact for any total; this keeps the spec's numbers readable).
pub const MAX_TOTAL: u64 = 1 << 32;
/// The salt of the pairing's stream of a game's seed (a stream of its own: `pairing`).
const SALT: u64 = 0x3E7A_6A3E_0176_D3C5;

/// One named 75 of a metagame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaDeck {
    pub name: Arc<str>,
    pub main: Vec<DefId>,
    pub sideboard: Vec<DefId>,
    /// The deck's choice table (ENGINE.md 168.3, 223; its list the deck's 75): played by the seat that plays this
    /// deck. None: that seat sideboards by its own decisions.
    pub table: Option<Arc<SideboardTable>>,
}

/// The spec's `"metagame"` (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metagame {
    pub decks: Vec<MetaDeck>,
    /// n x n, row-major: `weights[i * n + j]` the weight of (p0 = deck i, p1 = deck j).
    pub weights: Vec<u64>,
    /// The weights' sum (> 0).
    pub total: u64,
}

/// A match's pairing (`MatchState::pairing`): the index of each seat's deck in the spec's `metagame.decks`, and
/// their names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pairing {
    pub decks: [u8; 2],
    pub names: [Arc<str>; 2],
}

impl Pairing {
    /// `{"decks": [i, j], "names": [name_i, name_j]}` (the game log's, the game record's and the spectator view's).
    pub fn json(&self) -> Value {
        serde_json::json!({"decks": self.decks, "names": [&*self.names[0], &*self.names[1]]})
    }
}

/// The card ids of `names` (strings) in `db`: every card the table does not know or support named at once (no silent
/// drop), a token refused.
fn ids(db: &CardDb, x: &Value, what: &str) -> Result<Vec<DefId>, String> {
    let a = x.as_array().ok_or_else(|| format!("mtg spec: {what} must be a list of card names"))?;
    let mut out = Vec::with_capacity(a.len());
    let mut bad: Vec<String> = Vec::new();
    for n in a {
        let n = n.as_str().ok_or_else(|| format!("mtg spec: {what}: card names are strings"))?;
        match db.expect_id(n) {
            Ok(id) if db.def(id).token => return Err(format!("mtg spec: {what}: {n} is a token")),
            Ok(id) => out.push(id),
            Err(e) if !bad.contains(&e) => bad.push(e),
            Err(_) => {}
        }
    }
    match bad.len() {
        0 => Ok(out),
        1 => Err(format!("mtg spec: {what}: {}", bad.remove(0))),
        k => Err(format!("mtg spec: {what}: {k} cards the card table does not know or support: {}", bad.join("; "))),
    }
}

impl Metagame {
    /// The spec's `"metagame"` value (module docs) on card table `db` (constructed play's). The constructed deck rules
    /// and the format's are the spec's (`MtgSpec::parse`, which knows the format).
    pub fn parse(x: &Value, db: &CardDb) -> Result<Metagame, String> {
        let o = x.as_object().ok_or("mtg spec: metagame must be {\"decks\": [{\"name\", \"main\", \"sideboard\"}, ...], \"weights\": [[...], ...]} (ENGINE.md 176)")?;
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "decks" | "weights")) {
            return Err(format!("mtg spec: unknown metagame key {k:?} (decks, weights; ENGINE.md 176)"));
        }
        let list = o.get("decks").and_then(|v| v.as_array()).ok_or("mtg spec: metagame.decks must be a list of decks {\"name\", \"main\", \"sideboard\"} (ENGINE.md 176)")?;
        if list.is_empty() || list.len() > MAX_DECKS {
            return Err(format!("mtg spec: metagame.decks has {} decks: 1 to {MAX_DECKS} (ENGINE.md 176)", list.len()));
        }
        let mut decks: Vec<MetaDeck> = Vec::with_capacity(list.len());
        for (i, d) in list.iter().enumerate() {
            let e = d.as_object().ok_or_else(|| format!("mtg spec: metagame.decks[{i}] must be {{\"name\", \"main\", \"sideboard\"}}"))?;
            if let Some(k) = e.keys().find(|k| !matches!(k.as_str(), "name" | "main" | "sideboard" | "table")) {
                return Err(format!("mtg spec: unknown key {k:?} of metagame.decks[{i}] (name, main, sideboard, table)"));
            }
            let name = e.get("name").and_then(|v| v.as_str()).filter(|n| !n.trim().is_empty()).ok_or_else(|| format!("mtg spec: metagame.decks[{i}] needs a name (a non-empty string)"))?;
            if decks.iter().any(|x| &*x.name == name) {
                return Err(format!("mtg spec: metagame.decks names {name:?} twice (a name is the deck's in the game log)"));
            }
            let main = ids(db, e.get("main").ok_or_else(|| format!("mtg spec: metagame deck {name:?} has no main"))?, &format!("metagame deck {name:?}'s main"))?;
            let sideboard = match e.get("sideboard") {
                Some(v) => ids(db, v, &format!("metagame deck {name:?}'s sideboard"))?,
                None => Vec::new(),
            };
            // (ENGINE.md 240: the deck's own choice table, its list exactly this deck's 75 - the format is checked by the
            // spec, which knows it: a table is best-of-three's)
            let table = match e.get("table") {
                None => None,
                Some(x) => {
                    let t = SideboardTable::parse_as(x, db, &format!("metagame deck {name:?}'s table"))?;
                    if !t.fits(&MatchPool::constructed(&main, &sideboard).0) {
                        return Err(format!(
                            "mtg spec: metagame deck {name:?}'s table (version {:?}) is not of its 75: a deck's table lists exactly that deck's main and sideboard, its decks the main's size (ENGINE.md 240, 272; {} cards here, the table's {})",
                            t.version,
                            main.len(),
                            t.size
                        ));
                    }
                    Some(Arc::new(t))
                }
            };
            decks.push(MetaDeck { name: Arc::from(name), main, sideboard, table });
        }
        let n = decks.len();
        let weights = match o.get("weights") {
            None => vec![1; n * n],
            Some(w) => {
                let bad = || format!("mtg spec: metagame.weights must be a {n} x {n} matrix of non-negative integers (row: p0's deck, column: p1's; ENGINE.md 176), not {w}");
                let rows = w.as_array().filter(|r| r.len() == n).ok_or_else(bad)?;
                let mut out = Vec::with_capacity(n * n);
                for r in rows {
                    let r = r.as_array().filter(|r| r.len() == n).ok_or_else(bad)?;
                    for v in r {
                        out.push(v.as_u64().ok_or_else(bad)?);
                    }
                }
                out
            }
        };
        let total = weights.iter().try_fold(0u64, |t, &w| t.checked_add(w)).filter(|&t| t <= MAX_TOTAL);
        let total = match total {
            Some(0) => return Err("mtg spec: metagame.weights are all 0: no pairing to play (ENGINE.md 176)".into()),
            Some(t) => t,
            None => return Err(format!("mtg spec: metagame.weights sum past {MAX_TOTAL} (relative integers: e.g. 2 plays a cell twice as often as 1)")),
        };
        Ok(Metagame { decks, weights, total })
    }

    pub fn len(&self) -> usize {
        self.decks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.decks.is_empty()
    }

    /// The pairing (p0's deck, p1's deck) of the game (match) with seed `seed` (module docs): cell k with probability
    /// `weights[k] / total` exactly.
    pub fn pairing(&self, seed: u64) -> [u8; 2] {
        let mut r = Rng::new(split64(seed ^ SALT));
        // (an integer uniform below `total`, exactly: rejection of the top partial block of u64)
        let lim = u64::MAX - u64::MAX % self.total;
        let mut x = r.next_u64();
        while x >= lim {
            x = r.next_u64();
        }
        let mut x = x % self.total;
        let n = self.decks.len();
        for (k, &w) in self.weights.iter().enumerate() {
            if x < w {
                return [(k / n) as u8, (k % n) as u8];
            }
            x -= w;
        }
        unreachable!("x < total = the weights' sum")
    }

    /// The pairing's names (`MatchState::pairing`).
    pub fn named(&self, p: [u8; 2]) -> Pairing {
        Pairing { decks: p, names: p.map(|i| self.decks[i as usize].name.clone()) }
    }

    /// The seats' (main, sideboard) of pairing `p`.
    pub fn seats(&self, p: [u8; 2]) -> [(Vec<DefId>, Vec<DefId>); 2] {
        p.map(|i| {
            let d = &self.decks[i as usize];
            (d.main.clone(), d.sideboard.clone())
        })
    }

    /// The seats' choice tables of pairing `p` (ENGINE.md 240): each seat its deck's table, if the deck has one.
    pub fn tables(&self, p: [u8; 2]) -> [Option<Arc<SideboardTable>>; 2] {
        p.map(|i| self.decks[i as usize].table.clone())
    }

    /// Does any deck carry a choice table (ENGINE.md 240)?
    pub fn has_tables(&self) -> bool {
        self.decks.iter().any(|d| d.table.is_some())
    }

    /// The pairs of positive weight.
    pub fn played(&self) -> impl Iterator<Item = [u8; 2]> + '_ {
        let n = self.decks.len();
        self.weights.iter().enumerate().filter(|(_, &w)| w > 0).map(move |(k, _)| [(k / n) as u8, (k % n) as u8])
    }

    /// The spec `json` (a metagame spec) with pairing `p` as fixed `decks` lists instead of `metagame` - the spec
    /// whose game of any seed is the metagame spec's game of a seed with that pairing (module docs; the tests and the
    /// fuzz check it). Each seat's sideboard as `pN_sideboard` when it has one, its deck's choice table as `pN_table`
    /// when it has one (ENGINE.md 240: the table follows the deck to the seat that plays it; every other key of the
    /// spec - `dealt_decks`, `forced_find`, ... - as it is, but a `forced_find` neither of the pair's decks can ever
    /// apply to (no deck of the two holds both its spell and its card: in its 75 in best of three, its main in best of
    /// one - the engine's own rule, which refuses such a spec, ENGINE.md 180) is left out: in the metagame it is inert
    /// for that pairing's games - the spell never resolves, nothing of it is drawn from the game's RNG or encoded -, so
    /// the game is the same without it).
    pub fn pair_json(json: &str, p: [u8; 2]) -> Result<String, String> {
        let mut v: Value = serde_json::from_str(json).map_err(|e| format!("mtg spec: {e}"))?;
        let o = v.as_object_mut().ok_or("mtg spec: not a JSON object")?;
        let mg = o.remove("metagame").ok_or("mtg spec: no metagame")?;
        let list = mg.get("decks").and_then(|d| d.as_array()).ok_or("mtg spec: metagame.decks")?;
        let mut decks = serde_json::Map::new();
        for (s, &i) in p.iter().enumerate() {
            let d = list.get(i as usize).ok_or_else(|| format!("mtg spec: no metagame deck {i}"))?;
            decks.insert(format!("p{s}"), d["main"].clone());
            if d.get("sideboard").and_then(|x| x.as_array()).is_some_and(|a| !a.is_empty()) {
                decks.insert(format!("p{s}_sideboard"), d["sideboard"].clone());
            }
            if let Some(t) = d.get("table") {
                decks.insert(format!("p{s}_table"), t.clone());
            }
        }
        o.insert("decks".into(), Value::Object(decks));
        if let Some(ff) = o.get("forced_find") {
            let name = |k: &str| ff.get(k).and_then(|x| x.as_str()).map(str::to_owned);
            let (spell, card) = (name("spell"), name("card"));
            let bo3 = o.get("format").and_then(|f| f.as_str()) == Some("bo3");
            let holds = |i: u8, n: &str| {
                let d = &list[i as usize];
                let has = |k: &str| d.get(k).and_then(|x| x.as_array()).is_some_and(|a| a.iter().any(|c| c.as_str() == Some(n)));
                has("main") || (bo3 && has("sideboard"))
            };
            if let (Some(s), Some(c)) = (spell, card) {
                if !p.iter().any(|&i| holds(i, &s) && holds(i, &c)) {
                    o.remove("forced_find");
                }
            }
        }
        Ok(v.to_string())
    }
}
