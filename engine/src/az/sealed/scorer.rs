//! The deck evaluator's scorer (ENGINE.md 112): a trained `mtgz.sealed.evaluator` ensemble read from its exported
//! JSON (`mtgz.sealed.scorer`, format `mtgz-deck-scorer` version 1), scoring a 40-card deck's probability of winning
//! against the field exactly as the torch model does (f64 here, f32 there: within 1e-5, tested). The sealed spec's
//! deck choice by an evaluator (`DeckChoiceSpec::by`) and the hidden decklists' prior of such a field read it, so
//! self-play chooses decks with no Python callback.
//!
//! * **The file** carries everything a score needs: the card table (the evaluator's vocabulary, each card with the
//!   attributes the deck features read: land, basic, creature, mana value, colours, a basic's colour), the feature
//!   names (checked against [`FEATURES`]), the scaler, and per member the weights of its kind (`linear`: bias, the
//!   features' and the cards' weights; `set`: those, the card embeddings, the pairwise vectors and the MLP). The
//!   engine never re-derives an attribute from its own card data: the features are the Python ones by construction.
//! * **The score**: counts over the table (a deck card the table lacks is an error, never a dropped card); the 35
//!   deck features of `mtgz.sealed.evaluator.deck_features`, scaled; per member z = bias + Fz . wf + X . wc, for
//!   `set` + mlp([X @ emb / 40, Fz]) + 0.5 (|X @ u|^2 - X . |u|^2) / 40; P = the members' mean of sigmoid(z).
//! * **Its identity**: the file's sha256 ([`sha256`], computed here from the bytes read). A spec may pin it
//!   (`"evaluator_sha"`): a file with other contents is an error. The game log writes it beside every chosen deck.
//! * **Loading** reads the file once per process and path while its size and modification time stay the same
//!   ([`load`]; a run's versions are immutable files); the content hash is always the bytes'.
//! * **Kinds**: `linear` and `set`. Any other (the coming pairwise kind, "P(A beats B)" against a field summary,
//!   ENGINE.md 112) is refused loudly at load: [`Kind`] is where it goes.
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::db::CardDb;
use crate::ids::DefId;

/// The format this engine reads.
pub const FORMAT: &str = "mtgz-deck-scorer";
pub const VERSION: u64 = 1;

/// The deck features, in order (`mtgz.sealed.evaluator.FEATURES`): a file whose list differs is refused.
pub const FEATURES: [&str; 35] = [
    "lands", "nonbasic_lands", "creatures", "noncreature_spells", "mv<=1", "mv2", "mv3", "mv4", "mv5", "mv6+", "mean_mv", "colours",
    "splash_spells", "colourless_spells", "spells_W", "spells_U", "spells_B", "spells_R", "spells_G", "basics_W", "basics_U",
    "basics_B", "basics_R", "basics_G", "mono", "pair_WU", "pair_WB", "pair_WR", "pair_WG", "pair_UB", "pair_UR", "pair_UG",
    "pair_BR", "pair_BG", "pair_RG",
];
const NF: usize = FEATURES.len();
const COLOURS: &str = "WUBRG";

/// A card of the scorer's table: what the deck features read.
#[derive(Clone, Copy, Debug)]
struct Card {
    land: bool,
    basic: bool,
    creature: bool,
    mv: f64,
    /// WUBRG bits (bit i = `COLOURS[i]`).
    colours: u8,
    basic_colour: u8,
}

/// The model of every member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Logistic regression on the card counts and the deck features.
    Linear,
    /// The linear terms, DeepSets over card embeddings (dimension d) with the features through an MLP (`hidden`
    /// units), and the pairwise term (card vectors of dimension k).
    Set { d: usize, k: usize, hidden: usize },
}

/// One member's weights (dense, row-major).
struct Member {
    bias: f64,
    wf: Vec<f64>,
    wc: Vec<f64>,
    /// `set`: emb [V x d], u [V x k], |u_c|^2 [V], the MLP's first layer [hidden x (d + F)] and bias [hidden], its
    /// second layer [hidden] and bias.
    emb: Vec<f64>,
    u: Vec<f64>,
    uu: Vec<f64>,
    w1: Vec<f64>,
    b1: Vec<f64>,
    w2: Vec<f64>,
    b2: f64,
}

/// A loaded scorer (module docs).
pub struct Scorer {
    /// The file it was read from, and the sha256 of its bytes.
    pub path: String,
    pub sha256: [u8; 32],
    pub kind: Kind,
    cards: Vec<Card>,
    /// Per card definition of the EoE table: its row in the table (u16::MAX: none).
    index: Vec<u16>,
    mean: Vec<f64>,
    std: Vec<f64>,
    members: Vec<Member>,
}

impl std::fmt::Debug for Scorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scorer({} {:?}, {} members, sha256 {})", self.path, self.kind, self.members.len(), self.sha_hex())
    }
}

fn floats(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    let a = v.as_array().ok_or_else(|| format!("{what} must be a list of numbers"))?;
    a.iter().map(|x| x.as_f64().filter(|f| f.is_finite()).ok_or_else(|| format!("{what}: {x} is not a finite number"))).collect()
}

/// A matrix (a list of rows) of `rows` x `cols` as one row-major vector.
fn matrix(v: &Value, rows: usize, cols: usize, what: &str) -> Result<Vec<f64>, String> {
    let a = v.as_array().ok_or_else(|| format!("{what} must be a list of rows"))?;
    if a.len() != rows {
        return Err(format!("{what}: {} rows, not {rows}", a.len()));
    }
    let mut out = Vec::with_capacity(rows * cols);
    for (i, r) in a.iter().enumerate() {
        let r = floats(r, &format!("{what} row {i}"))?;
        if r.len() != cols {
            return Err(format!("{what} row {i}: {} columns, not {cols}", r.len()));
        }
        out.extend(r);
    }
    Ok(out)
}

fn colour_bits(s: &str, what: &str) -> Result<u8, String> {
    s.chars().try_fold(0u8, |m, ch| COLOURS.find(ch).map(|i| m | 1 << i).ok_or_else(|| format!("{what}: colour {ch:?} (WUBRG)")))
}

impl Scorer {
    /// The scorer of a file's bytes (`path` for messages and the record). Every check is loud: the format and
    /// version, a known kind, the features' names, the sizes, every card a card of `db`.
    pub fn parse(bytes: &[u8], path: &str, db: &CardDb) -> Result<Scorer, String> {
        let err = |e: String| format!("deck scorer {path}: {e}");
        let v: Value = serde_json::from_slice(bytes).map_err(|e| err(format!("not JSON: {e}")))?;
        let o = v.as_object().ok_or_else(|| err("not a JSON object".into()))?;
        if o.get("format").and_then(|x| x.as_str()) != Some(FORMAT) || o.get("version").and_then(|x| x.as_u64()) != Some(VERSION) {
            return Err(err(format!("not an {FORMAT} file of version {VERSION}")));
        }
        let names: Vec<&str> = o.get("features").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        if names != FEATURES {
            return Err(err(format!("its deck features {names:?} are not this engine's ({FEATURES:?})")));
        }
        let hyper = o.get("hyper").cloned().unwrap_or(Value::Null);
        let size = |k: &str| hyper.get(k).and_then(|x| x.as_u64()).map(|n| n as usize).ok_or_else(|| err(format!("hyper.{k} missing")));
        let kind = match o.get("kind").and_then(|x| x.as_str()) {
            Some("linear") => Kind::Linear,
            Some("set") => Kind::Set { d: size("d")?, k: size("k")?, hidden: size("hidden")? },
            Some(other) => return Err(err(format!("kind {other:?}: this engine scores \"linear\" and \"set\" (ENGINE.md 112)"))),
            None => return Err(err("kind missing".into())),
        };
        if o.get("combine").and_then(|x| x.as_str()) != Some("mean_sigmoid") {
            return Err(err("combine must be \"mean_sigmoid\"".into()));
        }
        // the card table
        let ca = o.get("cards").and_then(|x| x.as_array()).ok_or_else(|| err("cards missing".into()))?;
        let mut index = vec![u16::MAX; db.len()];
        let mut cards = Vec::with_capacity(ca.len());
        for (i, c) in ca.iter().enumerate() {
            let name = c.get("name").and_then(|x| x.as_str()).ok_or_else(|| err(format!("card {i}: no name")))?;
            let flag = |k: &str| match c.get(k).and_then(|x| x.as_u64()) {
                Some(0) => Ok(false),
                Some(1) => Ok(true),
                _ => Err(err(format!("card {name}: {k} must be 0 or 1"))),
            };
            let s = |k: &str| c.get(k).and_then(|x| x.as_str()).ok_or_else(|| err(format!("card {name}: {k} missing")));
            let card = Card {
                land: flag("land")?,
                basic: flag("basic")?,
                creature: flag("creature")?,
                mv: c.get("mv").and_then(|x| x.as_f64()).filter(|m| m.is_finite() && *m >= 0.0).ok_or_else(|| err(format!("card {name}: mv")))?,
                colours: colour_bits(s("colours")?, name).map_err(&err)?,
                basic_colour: colour_bits(s("basic_colour")?, name).map_err(&err)?,
            };
            let id = db.id(name).ok_or_else(|| err(format!("card {name:?} is not a card of the EoE table")))?;
            if index[id as usize] != u16::MAX {
                return Err(err(format!("card {name:?} twice in the table")));
            }
            index[id as usize] = i as u16;
            cards.push(card);
        }
        let nv = cards.len();
        let sc = o.get("scaler").ok_or_else(|| err("scaler missing".into()))?;
        let mean = floats(sc.get("mean").unwrap_or(&Value::Null), "scaler.mean").map_err(&err)?;
        let std = floats(sc.get("std").unwrap_or(&Value::Null), "scaler.std").map_err(&err)?;
        if mean.len() != NF || std.len() != NF || std.iter().any(|&s| s <= 0.0) {
            return Err(err(format!("the scaler must have {NF} means and {NF} positive sds")));
        }
        let ma = o.get("members").and_then(|x| x.as_array()).filter(|a| !a.is_empty()).ok_or_else(|| err("no members".into()))?;
        let mut members = Vec::with_capacity(ma.len());
        for (mi, m) in ma.iter().enumerate() {
            let w = |k: &str| m.get(k).unwrap_or(&Value::Null);
            let what = |k: &str| format!("member {mi} {k}");
            let bias = w("bias").as_f64().filter(|b| b.is_finite()).ok_or_else(|| err(what("bias")))?;
            let wf = floats(w("wf"), &what("wf")).map_err(&err)?;
            let wc = floats(w("wc"), &what("wc")).map_err(&err)?;
            if wf.len() != NF || wc.len() != nv {
                return Err(err(format!("member {mi}: wf {} (not {NF}) or wc {} (not {nv}) values", wf.len(), wc.len())));
            }
            let mut mem = Member { bias, wf, wc, emb: Vec::new(), u: Vec::new(), uu: Vec::new(), w1: Vec::new(), b1: Vec::new(), w2: Vec::new(), b2: 0.0 };
            if let Kind::Set { d, k, hidden } = kind {
                mem.emb = matrix(w("emb"), nv, d, &what("emb")).map_err(&err)?;
                mem.u = matrix(w("u"), nv, k, &what("u")).map_err(&err)?;
                mem.uu = (0..nv).map(|c| mem.u[c * k..(c + 1) * k].iter().map(|x| x * x).sum()).collect();
                let mlp = w("mlp").as_array().filter(|a| a.len() == 2).ok_or_else(|| err(what("mlp: two layers")))?;
                mem.w1 = matrix(&mlp[0]["w"], hidden, d + NF, &what("mlp[0].w")).map_err(&err)?;
                mem.b1 = floats(&mlp[0]["b"], &what("mlp[0].b")).map_err(&err)?;
                mem.w2 = matrix(&mlp[1]["w"], 1, hidden, &what("mlp[1].w")).map_err(&err)?;
                let b2 = floats(&mlp[1]["b"], &what("mlp[1].b")).map_err(&err)?;
                if mem.b1.len() != hidden || b2.len() != 1 {
                    return Err(err(format!("member {mi}: the MLP's biases ({} and {}, not {hidden} and 1)", mem.b1.len(), b2.len())));
                }
                mem.b2 = b2[0];
            }
            members.push(mem);
        }
        Ok(Scorer { path: path.to_string(), sha256: sha256(bytes), kind, cards, index, mean, std, members })
    }

    /// The sha256 of the file, in hex.
    pub fn sha_hex(&self) -> String {
        hex(&self.sha256)
    }

    /// The members.
    pub fn members(&self) -> usize {
        self.members.len()
    }

    /// The cards of the table.
    pub fn cards(&self) -> usize {
        self.cards.len()
    }

    /// Whether the table has card `d` (a definition of the EoE table).
    pub fn has(&self, d: DefId) -> bool {
        self.index.get(d as usize).is_some_and(|&i| i != u16::MAX)
    }

    /// A deck's probability of winning against the field (module docs). A card outside the table is an error.
    pub fn score(&self, deck: &[DefId]) -> Result<f64, String> {
        // the deck as (row, count), sorted by row
        let mut rows: Vec<(usize, f64)> = Vec::with_capacity(deck.len());
        for &d in deck {
            let i = *self.index.get(d as usize).filter(|&&i| i != u16::MAX).ok_or_else(|| {
                format!("deck scorer {}: card definition {d} is not in its table", self.path)
            })? as usize;
            match rows.binary_search_by_key(&i, |r| r.0) {
                Ok(at) => rows[at].1 += 1.0,
                Err(at) => rows.insert(at, (i, 1.0)),
            }
        }
        let f = self.features(&rows);
        let fz: Vec<f64> = (0..NF).map(|j| (f[j] - self.mean[j]) / self.std[j]).collect();
        let mut p = 0.0;
        for m in &self.members {
            let mut z = m.bias + (0..NF).map(|j| fz[j] * m.wf[j]).sum::<f64>() + rows.iter().map(|&(i, x)| x * m.wc[i]).sum::<f64>();
            if let Kind::Set { d, k, hidden } = self.kind {
                // DeepSets: h = X @ emb / 40, then the MLP of [h, Fz]
                let mut x = vec![0.0; d + NF];
                for &(i, c) in &rows {
                    for (j, h) in x[..d].iter_mut().enumerate() {
                        *h += c * m.emb[i * d + j];
                    }
                }
                for h in &mut x[..d] {
                    *h /= 40.0;
                }
                x[d..].copy_from_slice(&fz);
                let mut out = m.b2;
                for (r, (&b, &w2)) in m.b1.iter().zip(&m.w2).enumerate() {
                    let row = &m.w1[r * (d + NF)..(r + 1) * (d + NF)];
                    let a = b + row.iter().zip(&x).map(|(w, v)| w * v).sum::<f64>();
                    out += w2 * a.max(0.0);
                }
                debug_assert_eq!(m.b1.len(), hidden);
                z += out;
                // the pairwise term: 0.5 (|X @ u|^2 - X . |u|^2) / 40
                let mut s = vec![0.0; k];
                for &(i, c) in &rows {
                    for (j, sj) in s.iter_mut().enumerate() {
                        *sj += c * m.u[i * k + j];
                    }
                }
                let self_terms: f64 = rows.iter().map(|&(i, c)| c * m.uu[i]).sum();
                z += 0.5 * (s.iter().map(|x| x * x).sum::<f64>() - self_terms) / 40.0;
            }
            p += 1.0 / (1.0 + (-z).exp());
        }
        Ok(p / self.members.len() as f64)
    }

    /// The deck features of `mtgz.sealed.evaluator.deck_features` (unscaled) from the table's attributes.
    fn features(&self, rows: &[(usize, f64)]) -> [f64; NF] {
        let mut f = [0.0; NF];
        let (mut spells, mut creatures, mut mv_sum, mut colourless) = (0.0, 0.0, 0.0, 0.0);
        let mut per = [0.0f64; 5];
        let mut basics = [0.0f64; 5];
        let mut curve = [0.0f64; 6];
        for &(i, x) in rows {
            let c = &self.cards[i];
            if c.land {
                f[0] += x;
                if !c.basic {
                    f[1] += x;
                }
            } else {
                spells += x;
                if c.creature {
                    creatures += x;
                }
                mv_sum += x * c.mv;
                let b = if c.mv <= 1.0 { 0 } else if c.mv == 2.0 { 1 } else if c.mv == 3.0 { 2 } else if c.mv == 4.0 { 3 } else if c.mv == 5.0 { 4 } else if c.mv >= 6.0 { 5 } else { 6 };
                if b < 6 {
                    curve[b] += x;
                }
                if c.colours == 0 {
                    colourless += x;
                }
                for (j, p) in per.iter_mut().enumerate() {
                    if c.colours & (1 << j) != 0 {
                        *p += x;
                    }
                }
            }
            for (j, b) in basics.iter_mut().enumerate() {
                if c.basic_colour & (1 << j) != 0 {
                    *b += x;
                }
            }
        }
        // the main colours: the two with the most spells (a stable sort: the first in WUBRG order on a tie), with
        // spells at all
        let mut order = [0usize, 1, 2, 3, 4];
        order.sort_by(|&a, &b| per[b].total_cmp(&per[a]));
        let mut main = 0u8;
        for &j in &order[..2] {
            if per[j] > 0.0 {
                main |= 1 << j;
            }
        }
        let splash: f64 = rows.iter().filter(|&&(i, _)| !self.cards[i].land && self.cards[i].colours & !main != 0).map(|&(_, x)| x).sum();
        f[2] = creatures;
        f[3] = spells - creatures;
        f[4..10].copy_from_slice(&curve);
        f[10] = mv_sum / spells.max(1.0);
        f[11] = per.iter().filter(|&&p| p > 0.0).count() as f64;
        f[12] = splash;
        f[13] = colourless;
        f[14..19].copy_from_slice(&per);
        f[19..24].copy_from_slice(&basics);
        f[24] = (main.count_ones() == 1) as u8 as f64;
        let mut k = 25;
        for a in 0..5 {
            for b in a + 1..5 {
                f[k] = (main & (1 << a) != 0 && main & (1 << b) != 0) as u8 as f64;
                k += 1;
            }
        }
        f
    }
}

/// The scorer of the file at `path`, read once per process while the file's size and modification time stay the
/// same (module docs); `db` is the EoE table. An unreadable or invalid file is an error.
pub fn load(path: &str, db: &CardDb) -> Result<Arc<Scorer>, String> {
    static CACHE: Mutex<Vec<(String, u64, Option<std::time::SystemTime>, Arc<Scorer>)>> = Mutex::new(Vec::new());
    let meta = std::fs::metadata(path).map_err(|e| format!("deck scorer {path}: {e}"))?;
    let (len, mtime) = (meta.len(), meta.modified().ok());
    {
        let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(x) = c.iter().find(|x| x.0 == path && x.1 == len && x.2 == mtime && mtime.is_some()) {
            return Ok(x.3.clone());
        }
    }
    let bytes = std::fs::read(path).map_err(|e| format!("deck scorer {path}: {e}"))?;
    let s = Arc::new(Scorer::parse(&bytes, path, db)?);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    // (another thread read the same file meanwhile: its scorer is the one)
    if let Some(x) = c.iter().find(|x| x.0 == path && x.1 == len && x.2 == mtime && mtime.is_some() && x.3.sha256 == s.sha256) {
        return Ok(x.3.clone());
    }
    c.retain(|x| x.0 != path);
    if c.len() >= 64 {
        c.remove(0);
    }
    c.push((path.to_string(), len, mtime, s.clone()));
    Ok(s)
}

/// Lower-case hex of bytes.
pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A sha256 hex string (64 characters) as bytes.
pub fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// SHA-256 (FIPS 180-4) of `data`: the file's content hash (no dependency for one function; checked against the
/// standard's vectors and Python's hashlib).
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be,
        0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa,
        0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85,
        0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (x, y) in h.iter_mut().zip(v) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_meets_the_standards_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let a = vec![b'a'; 1_000_000];
        assert_eq!(hex(&sha256(&a)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
        assert_eq!(parse_hex32(&hex(&sha256(b"abc"))), Some(sha256(b"abc")));
        assert_eq!(parse_hex32("xyz"), None);
    }
}
