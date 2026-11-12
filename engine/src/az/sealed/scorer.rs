//! The deck evaluator's scorer (ENGINE.md 112): a trained `mtgz.sealed.evaluator` ensemble read from its torch-free
//! export (`Evaluator.export`, `"format": "mtgz-deck-evaluator-export"` version 1, specified in that module's docstring
//! and ENGINE.md 111.6), scoring a 40-card deck's probability of winning against the field exactly as the export's
//! numpy reference (`ReferenceScorer`) and the torch model do (f64 here; tested). The sealed spec's deck choice by an
//! evaluator (`DeckChoiceSpec::by`) and the hidden decklists' prior of such a field read it, so self-play chooses
//! decks with no Python callback.
//!
//! * **The file** carries everything a score needs: the card table in the EoE registry's order (`eoe_cards.txt`; a
//!   card `in_table` 0 - Paradox Haze - is never in a deck) with the attributes the deck features read (land, basic,
//!   creature, mana value, colours, a basic's colour), the feature names (checked against [`FEATURES`]) and scaler,
//!   and per member the weights of its kind. The engine never re-derives an attribute from its own card data: the
//!   features are the export's by construction.
//! * **The kinds** ([`Kind`]): `linear` (z = bias + wc . x + wf . f), `set` (+ the MLP of [x emb / 40, f] and the
//!   pairwise term 0.5 (|x u|^2 - x . |u|^2) / 40), each P = the members' mean of sigmoid(z): P(the deck wins against
//!   the field its training games had); `pair` (P(A beats B) = sigmoid(s(A) - s(B) + e(A) . W e(B)), s a linear or set
//!   strength without bias, e(x) = x U / 40 + f P), scored **against the export's stored field** B_1..B_N: the members'
//!   mean of (1/N) sum_j sigmoid(s(A) - s(B_j) + e(A) . g_j), s(B_j) and g_j = W e(B_j) computed once at load. A pair
//!   export without a stored field is refused (a named field would be a file of its own, pinned like the evaluator:
//!   not built, ENGINE.md 112.7).
//! * **The score**: counts over the table (a deck card the table lacks is an error, never a dropped card); the 35 deck
//!   features (`mtgz.sealed.evaluator.deck_features`, the export's formulas: the main pair a stable sort, ties to the
//!   earlier colour), scaled; then the kind's formula.
//! * **Its identity**: the file's sha256 ([`sha256`], computed here from the bytes read). A spec may pin it
//!   (`"evaluator_sha"`): a file with other contents is an error. The game log writes it beside every chosen deck.
//! * **Loading** reads the file once per process and path while its size and modification time stay the same
//!   ([`load`]; a run's versions are immutable files); the content hash is always the bytes'. Everything malformed -
//!   another format or version, an unknown kind, other feature names, a size, a non-0/1 flag, a card of the table not
//!   in the EoE table or twice - is refused loudly.
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::db::CardDb;
use crate::ids::DefId;

/// The format this engine reads (`mtgz.sealed.evaluator.EXPORT_FORMAT`).
pub const FORMAT: &str = "mtgz-deck-evaluator-export";
pub const VERSION: u64 = 1;

/// The deck features, in order (`mtgz.sealed.evaluator.FEATURES`): a file whose list differs is refused.
pub const FEATURES: [&str; 35] = [
    "lands", "nonbasic_lands", "creatures", "noncreature_spells", "mv<=1", "mv2", "mv3", "mv4", "mv5", "mv6+", "mean_mv", "colours",
    "splash_spells", "colourless_spells", "spells_W", "spells_U", "spells_B", "spells_R", "spells_G", "basics_W", "basics_U",
    "basics_B", "basics_R", "basics_G", "mono", "pair_WU", "pair_WB", "pair_WR", "pair_WG", "pair_UB", "pair_UR", "pair_UG",
    "pair_BR", "pair_BG", "pair_RG",
];
const NF: usize = FEATURES.len();

/// A card of the export's table: what the deck features read.
#[derive(Clone, Copy, Debug)]
struct Card {
    land: bool,
    basic: bool,
    creature: bool,
    mv: f64,
    /// WUBRG bits (bit i: the export's column i).
    colours: u8,
    basic_colour: u8,
}

/// A deck model's architecture: the linear terms, or with the set network's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Linear,
    /// Card embeddings of dimension d, pairwise card vectors of dimension k, an MLP of `hidden` units.
    Set { d: usize, k: usize, hidden: usize },
}

/// The export's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Linear,
    Set { d: usize, k: usize, hidden: usize },
    /// P(A beats B) from a strength (`base`, no bias) and a matchup term of `dim` dimensions (0: Bradley-Terry),
    /// scored against the stored field.
    Pair { base: Base, dim: usize },
}

/// A linear or set member (a pair member's strength: bias 0).
struct DeckModel {
    bias: f64,
    wf: Vec<f64>,
    wc: Vec<f64>,
    set: Option<SetPart>,
}

/// The set network's weights (row-major): emb [C x d], u [C x k], |u_c|^2 [C], w1 [hidden x (d + F)], b1, w2, b2.
struct SetPart {
    d: usize,
    k: usize,
    emb: Vec<f64>,
    u: Vec<f64>,
    uu: Vec<f64>,
    w1: Vec<f64>,
    b1: Vec<f64>,
    w2: Vec<f64>,
    b2: f64,
}

/// A pair member: its strength, the embedding's U [C x dim] and P [F x dim], and the stored field precomputed: per field
/// deck j, s(B_j) and g_j = W e(B_j) [N x dim].
struct PairMember {
    strength: DeckModel,
    dim: usize,
    u: Vec<f64>,
    p: Vec<f64>,
    field_s: Vec<f64>,
    field_g: Vec<f64>,
}

enum Members {
    Deck(Vec<DeckModel>),
    Pair(Vec<PairMember>),
}

/// A loaded scorer (module docs).
pub struct Scorer {
    /// The file it was read from, and the sha256 of its bytes.
    pub path: String,
    pub sha256: [u8; 32],
    pub kind: Kind,
    cards: Vec<Card>,
    /// Per card definition of the EoE table: its row in the export's table (u16::MAX: none).
    index: Vec<u16>,
    mean: Vec<f64>,
    std: Vec<f64>,
    members: Members,
    /// The stored field's decks (pair; 0 otherwise).
    field_n: usize,
}

impl std::fmt::Debug for Scorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scorer({} {:?}, {} members, sha256 {})", self.path, self.kind, self.members(), self.sha_hex())
    }
}

fn floats(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    let a = v.as_array().ok_or_else(|| format!("{what} must be a list of numbers"))?;
    a.iter().map(|x| x.as_f64().filter(|f| f.is_finite()).ok_or_else(|| format!("{what}: {x} is not a finite number"))).collect()
}

fn floats_n(v: &Value, n: usize, what: &str) -> Result<Vec<f64>, String> {
    let x = floats(v, what)?;
    if x.len() != n {
        return Err(format!("{what}: {} values, not {n}", x.len()));
    }
    Ok(x)
}

/// A matrix (a list of rows) of `rows` x `cols` as one row-major vector.
fn matrix(v: &Value, rows: usize, cols: usize, what: &str) -> Result<Vec<f64>, String> {
    let a = v.as_array().ok_or_else(|| format!("{what} must be a list of rows"))?;
    if a.len() != rows {
        return Err(format!("{what}: {} rows, not {rows}", a.len()));
    }
    let mut out = Vec::with_capacity(rows * cols);
    for (i, r) in a.iter().enumerate() {
        out.extend(floats_n(r, cols, &format!("{what} row {i}"))?);
    }
    Ok(out)
}

fn scalar(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64().filter(|f| f.is_finite()).ok_or_else(|| format!("{what} must be a finite number"))
}

/// A 0/1 flag of the card table.
fn flag(x: f64, what: &str) -> Result<bool, String> {
    if x == 0.0 {
        Ok(false)
    } else if x == 1.0 {
        Ok(true)
    } else {
        Err(format!("{what}: {x}, not 0 or 1"))
    }
}

fn size(h: &Value, k: &str) -> Result<usize, String> {
    h.get(k).and_then(|x| x.as_u64()).map(|n| n as usize).ok_or_else(|| format!("hyper.{k} missing"))
}

fn set_base(h: &Value) -> Result<Base, String> {
    Ok(Base::Set { d: size(h, "d")?, k: size(h, "k")?, hidden: size(h, "hidden")? })
}

impl DeckModel {
    /// A linear or set member of `base` over `nc` cards (`bias`: the key is read; else 0, a pair's strength).
    fn parse(m: &Value, base: Base, nc: usize, bias: bool, what: &str) -> Result<DeckModel, String> {
        let w = |k: &str| m.get(k).unwrap_or(&Value::Null);
        let b = if bias { scalar(w("bias"), &format!("{what} bias"))? } else { 0.0 };
        if !bias && m.get("bias").is_some() {
            return Err(format!("{what}: a pair's strength has no bias"));
        }
        let wf = floats_n(w("wf"), NF, &format!("{what} wf"))?;
        let wc = floats_n(w("wc"), nc, &format!("{what} wc"))?;
        let set = match base {
            Base::Linear => {
                if m.get("emb").is_some() {
                    return Err(format!("{what}: set weights in a linear member"));
                }
                None
            }
            Base::Set { d, k, hidden } => {
                let u = matrix(w("u"), nc, k, &format!("{what} u"))?;
                let uu = (0..nc).map(|c| u[c * k..(c + 1) * k].iter().map(|x| x * x).sum()).collect();
                Some(SetPart {
                    d,
                    k,
                    emb: matrix(w("emb"), nc, d, &format!("{what} emb"))?,
                    u,
                    uu,
                    w1: matrix(w("w1"), hidden, d + NF, &format!("{what} w1"))?,
                    b1: floats_n(w("b1"), hidden, &format!("{what} b1"))?,
                    w2: floats_n(w("w2"), hidden, &format!("{what} w2"))?,
                    b2: scalar(w("b2"), &format!("{what} b2"))?,
                })
            }
        };
        Ok(DeckModel { bias: b, wf, wc, set })
    }

    /// z of a deck (`rows`: (card row, count), `fz`: its scaled features).
    fn z(&self, rows: &[(usize, f64)], fz: &[f64]) -> f64 {
        let mut z = self.bias + (0..NF).map(|j| fz[j] * self.wf[j]).sum::<f64>() + rows.iter().map(|&(i, x)| x * self.wc[i]).sum::<f64>();
        if let Some(s) = &self.set {
            let (d, k) = (s.d, s.k);
            // DeepSets: h = x emb / 40, then the MLP of [h, f]
            let mut x = vec![0.0; d + NF];
            for &(i, c) in rows {
                for (j, h) in x[..d].iter_mut().enumerate() {
                    *h += c * s.emb[i * d + j];
                }
            }
            for h in &mut x[..d] {
                *h /= 40.0;
            }
            x[d..].copy_from_slice(fz);
            let mut out = s.b2;
            for (r, (&b, &w2)) in s.b1.iter().zip(&s.w2).enumerate() {
                let row = &s.w1[r * (d + NF)..(r + 1) * (d + NF)];
                out += w2 * (b + row.iter().zip(&x).map(|(w, v)| w * v).sum::<f64>()).max(0.0);
            }
            z += out;
            // the pairwise term: 0.5 (|x u|^2 - x . |u|^2) / 40
            let mut su = vec![0.0; k];
            for &(i, c) in rows {
                for (j, v) in su.iter_mut().enumerate() {
                    *v += c * s.u[i * k + j];
                }
            }
            let own: f64 = rows.iter().map(|&(i, c)| c * s.uu[i]).sum();
            z += 0.5 * (su.iter().map(|v| v * v).sum::<f64>() - own) / 40.0;
        }
        z
    }
}

impl PairMember {
    /// e(x) = x U / 40 + f P (`dim` floats).
    fn embed(&self, rows: &[(usize, f64)], fz: &[f64]) -> Vec<f64> {
        let dim = self.dim;
        let mut e = vec![0.0; dim];
        for &(i, c) in rows {
            for (j, v) in e.iter_mut().enumerate() {
                *v += c * self.u[i * dim + j];
            }
        }
        for v in &mut e {
            *v /= 40.0;
        }
        for (f, &x) in fz.iter().enumerate() {
            for (j, v) in e.iter_mut().enumerate() {
                *v += x * self.p[f * dim + j];
            }
        }
        e
    }
}

fn sigmoid(z: f64) -> f64 {
    1.0 / (1.0 + (-z).exp())
}

impl Scorer {
    /// The scorer of an export's bytes (`path` for messages and the record; `db` the EoE table). Every check is loud
    /// (module docs).
    pub fn parse(bytes: &[u8], path: &str, db: &CardDb) -> Result<Scorer, String> {
        let err = |e: String| format!("deck evaluator {path}: {e}");
        let v: Value = serde_json::from_slice(bytes).map_err(|e| err(format!("not JSON: {e}")))?;
        let o = v.as_object().ok_or_else(|| err("not a JSON object".into()))?;
        if o.get("format").and_then(|x| x.as_str()) != Some(FORMAT) || o.get("version").and_then(|x| x.as_u64()) != Some(VERSION) {
            return Err(err(format!("not an {FORMAT} file of version {VERSION} (mtgz.sealed.evaluator's Evaluator.export)")));
        }
        let hyper = o.get("hyper").cloned().unwrap_or(Value::Null);
        let kind = match o.get("kind").and_then(|x| x.as_str()) {
            Some("linear") => Kind::Linear,
            Some("set") => match set_base(&hyper).map_err(&err)? {
                Base::Set { d, k, hidden } => Kind::Set { d, k, hidden },
                Base::Linear => unreachable!(),
            },
            Some("pair") => {
                let base = match hyper.get("base").and_then(|x| x.as_str()) {
                    Some("linear") => Base::Linear,
                    Some("set") => set_base(&hyper).map_err(&err)?,
                    b => return Err(err(format!("pair: hyper.base {b:?} (linear or set)"))),
                };
                Kind::Pair { base, dim: size(&hyper, "dim").map_err(&err)? }
            }
            Some(other) => return Err(err(format!("kind {other:?}: this engine scores \"linear\", \"set\" and \"pair\""))),
            None => return Err(err("kind missing".into())),
        };
        // the card table, in the registry's order
        let c = o.get("cards").ok_or_else(|| err("cards missing".into()))?;
        let names: Vec<&str> = c.get("names").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        let nc = names.len();
        if nc == 0 || nc >= u16::MAX as usize {
            return Err(err("cards.names: a list of card names".into()));
        }
        let col = |k: &str| floats_n(c.get(k).unwrap_or(&Value::Null), nc, &format!("cards.{k}")).map_err(&err);
        let (in_table, land, basic, creature, mv) = (col("in_table")?, col("land")?, col("basic")?, col("creature")?, col("mv")?);
        let colours = matrix(c.get("colours").unwrap_or(&Value::Null), nc, 5, "cards.colours").map_err(&err)?;
        let basic_colour = matrix(c.get("basic_colour").unwrap_or(&Value::Null), nc, 5, "cards.basic_colour").map_err(&err)?;
        let mut index = vec![u16::MAX; db.len()];
        let mut cards = Vec::with_capacity(nc);
        for (i, &name) in names.iter().enumerate() {
            let bits = |m: &[f64]| -> Result<u8, String> {
                (0..5).try_fold(0u8, |b, j| flag(m[i * 5 + j], &format!("card {name} colour {j}")).map(|on| b | (on as u8) << j))
            };
            cards.push(Card {
                land: flag(land[i], &format!("card {name} land")).map_err(&err)?,
                basic: flag(basic[i], &format!("card {name} basic")).map_err(&err)?,
                creature: flag(creature[i], &format!("card {name} creature")).map_err(&err)?,
                mv: Some(mv[i]).filter(|m| *m >= 0.0).ok_or_else(|| err(format!("card {name}: mv {}", mv[i])))?,
                colours: bits(&colours).map_err(&err)?,
                basic_colour: bits(&basic_colour).map_err(&err)?,
            });
            if flag(in_table[i], &format!("card {name} in_table")).map_err(&err)? {
                let id = db.id(name).ok_or_else(|| err(format!("card {name:?} is in its table but not a card of the EoE table")))?;
                if index[id as usize] != u16::MAX {
                    return Err(err(format!("card {name:?} twice in the table")));
                }
                index[id as usize] = i as u16;
            }
        }
        // the features and their scaler
        let f = o.get("features").ok_or_else(|| err("features missing".into()))?;
        let fnames: Vec<&str> = f.get("names").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
        if fnames != FEATURES {
            return Err(err(format!("its deck features {fnames:?} are not this engine's ({FEATURES:?})")));
        }
        let mean = floats_n(f.get("mean").unwrap_or(&Value::Null), NF, "features.mean").map_err(&err)?;
        let std = floats_n(f.get("std").unwrap_or(&Value::Null), NF, "features.std").map_err(&err)?;
        if std.iter().any(|&s| s <= 0.0) {
            return Err(err("features.std: every sd positive".into()));
        }
        let ma = o.get("members").and_then(|x| x.as_array()).filter(|a| !a.is_empty()).ok_or_else(|| err("no members".into()))?;
        let mut s = Scorer { path: path.to_string(), sha256: sha256(bytes), kind, cards, index, mean, std, members: Members::Deck(Vec::new()), field_n: 0 };
        let mut field_n = 0;
        let members = match kind {
            Kind::Linear | Kind::Set { .. } => {
                let base = match kind {
                    Kind::Set { d, k, hidden } => Base::Set { d, k, hidden },
                    _ => Base::Linear,
                };
                Members::Deck(ma.iter().enumerate().map(|(i, m)| DeckModel::parse(m, base, nc, true, &format!("member {i}"))).collect::<Result<_, _>>().map_err(&err)?)
            }
            Kind::Pair { base, dim } => {
                // the stored field, as counts over the table
                let fd = o.get("field").filter(|x| !x.is_null()).ok_or_else(|| {
                    err("a pair evaluator without a stored field: the engine scores against it (ENGINE.md 112)".into())
                })?;
                let decks = fd.get("decks").and_then(|x| x.as_array()).filter(|a| !a.is_empty()).ok_or_else(|| err("field.decks: a list of decks".into()))?;
                let mut field: Vec<Vec<(usize, f64)>> = Vec::with_capacity(decks.len());
                for (j, dk) in decks.iter().enumerate() {
                    let mut rows: Vec<(usize, f64)> = Vec::new();
                    for e in dk.as_array().ok_or_else(|| err(format!("field deck {j}: a list of [card id, count]")))? {
                        let (cid, k) = (e.get(0).and_then(|x| x.as_u64()), e.get(1).and_then(|x| x.as_f64()));
                        let (cid, k) = cid.zip(k).filter(|(c, k)| (*c as usize) < nc && *k > 0.0).ok_or_else(|| err(format!("field deck {j}: {e}")))?;
                        if in_table[cid as usize] != 1.0 {
                            return Err(err(format!("field deck {j}: {} is not in the table", names[cid as usize])));
                        }
                        match rows.binary_search_by_key(&(cid as usize), |r| r.0) {
                            Ok(at) => rows[at].1 += k,
                            Err(at) => rows.insert(at, (cid as usize, k)),
                        }
                    }
                    field.push(rows);
                }
                let fz: Vec<[f64; NF]> = field.iter().map(|r| s.scaled(r)).collect();
                let mut out = Vec::with_capacity(ma.len());
                for (i, m) in ma.iter().enumerate() {
                    let what = format!("member {i}");
                    let strength = DeckModel::parse(m.get("strength").unwrap_or(&Value::Null), base, nc, false, &format!("{what} strength")).map_err(&err)?;
                    let (u, p, w) = if dim > 0 {
                        (
                            matrix(m.get("U").unwrap_or(&Value::Null), nc, dim, &format!("{what} U")).map_err(&err)?,
                            matrix(m.get("P").unwrap_or(&Value::Null), NF, dim, &format!("{what} P")).map_err(&err)?,
                            matrix(m.get("W").unwrap_or(&Value::Null), dim, dim, &format!("{what} W")).map_err(&err)?,
                        )
                    } else {
                        if m.get("U").is_some() || m.get("W").is_some() {
                            return Err(err(format!("{what}: matchup weights with dim 0")));
                        }
                        (Vec::new(), Vec::new(), Vec::new())
                    };
                    let mut pm = PairMember { strength, dim, u, p, field_s: Vec::with_capacity(field.len()), field_g: Vec::with_capacity(field.len() * dim) };
                    for (rows, f) in field.iter().zip(&fz) {
                        pm.field_s.push(pm.strength.z(rows, f));
                        if dim > 0 {
                            let e = pm.embed(rows, f);
                            pm.field_g.extend((0..dim).map(|r| (0..dim).map(|c| w[r * dim + c] * e[c]).sum::<f64>()));
                        }
                    }
                    out.push(pm);
                }
                field_n = field.len();
                Members::Pair(out)
            }
        };
        s.members = members;
        s.field_n = field_n;
        Ok(s)
    }

    /// The sha256 of the file, in hex.
    pub fn sha_hex(&self) -> String {
        hex(&self.sha256)
    }

    /// The members.
    pub fn members(&self) -> usize {
        match &self.members {
            Members::Deck(v) => v.len(),
            Members::Pair(v) => v.len(),
        }
    }

    /// The cards of the table (in it or not), and the stored field's decks (pair; else 0).
    pub fn cards(&self) -> usize {
        self.cards.len()
    }

    pub fn field_decks(&self) -> usize {
        self.field_n
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
                format!("deck evaluator {}: card definition {d} is not in its table", self.path)
            })? as usize;
            match rows.binary_search_by_key(&i, |r| r.0) {
                Ok(at) => rows[at].1 += 1.0,
                Err(at) => rows.insert(at, (i, 1.0)),
            }
        }
        let fz = self.scaled(&rows);
        Ok(match &self.members {
            Members::Deck(v) => v.iter().map(|m| sigmoid(m.z(&rows, &fz))).sum::<f64>() / v.len() as f64,
            Members::Pair(v) => {
                let mut p = 0.0;
                for m in v {
                    let s = m.strength.z(&rows, &fz);
                    let e = if m.dim > 0 { m.embed(&rows, &fz) } else { Vec::new() };
                    let mut q = 0.0;
                    for j in 0..m.field_s.len() {
                        let g = &m.field_g[j * m.dim..(j + 1) * m.dim];
                        q += sigmoid(s - m.field_s[j] + e.iter().zip(g).map(|(a, b)| a * b).sum::<f64>());
                    }
                    p += q / m.field_s.len() as f64;
                }
                p / v.len() as f64
            }
        })
    }

    /// The scaled deck features f of `rows`.
    fn scaled(&self, rows: &[(usize, f64)]) -> [f64; NF] {
        let f = self.features(rows);
        let mut out = [0.0; NF];
        for j in 0..NF {
            out[j] = (f[j] - self.mean[j]) / self.std[j];
        }
        out
    }

    /// The deck features (the export's formulas; unscaled) from the table's attributes.
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
    let meta = std::fs::metadata(path).map_err(|e| format!("deck evaluator {path}: {e}"))?;
    let (len, mtime) = (meta.len(), meta.modified().ok());
    {
        let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(x) = c.iter().find(|x| x.0 == path && x.1 == len && x.2 == mtime && mtime.is_some()) {
            return Ok(x.3.clone());
        }
    }
    let bytes = std::fs::read(path).map_err(|e| format!("deck evaluator {path}: {e}"))?;
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
