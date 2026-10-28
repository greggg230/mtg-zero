//! Stable card identities for the network input (ENGINE.md 22.2): the index a card has in
//! `encode` (its card code, the bags, the token network's identity embedding) must not change
//! when the engine supports more cards. The card table (`CardDb`: the supported cards in IR
//! order, then the tokens they create in the order lowering meets them) renumbers as support
//! grows, so the encoding does not use it:
//!
//! * a cube card's identity is its position in the PowerLSV cube's IR card list
//!   (`data/cards/powerlsv.ir.json`: all 546 cards of the cube, supported or not, sorted by
//!   name; basic lands included): 0..545, capacity [`CUBE_CAP`];
//! * a token's identity is `CUBE_CAP` + its line in the token registry `tokens.txt` (one key per
//!   token kind: name, power / toughness, colours, card types, subtypes, keywords). New tokens
//!   are appended, never inserted (`MTGZ_WRITE_TOKENS=1 cargo test -p engine --test az
//!   token_registry` appends the missing ones; the test fails while a token of the cube's card
//!   table has no line);
//! * an emblem (CR 114) has the identity of the card that creates it (its definition is named
//!   "<card> emblem"); the encoding keeps emblems in bags of their own;
//! * a face (a double-faced card's back face, an Adventure: CR 712, 715) has its card's
//!   identity (the card code's descriptor shows the face that is up);
//! * a rules definition (`CardDef::rules`: the Undercity dungeon, the initiative's abilities;
//!   ENGINE.md 27.4) is in the token registry under the key `<name> rules`;
//! * a legacy card (outside the cube, played in constructed decks: `CardDb::constructed`,
//!   ENGINE.md 54): [`LEGACY_BASE`] + its line in the legacy registry `legacy_cards.txt` (one
//!   card name per line, appended, never inserted or reordered: 546.. in the cube's free range
//!   below [`CUBE_CAP`], so layout v4 is unchanged); `cargo test -p engine --test az
//!   legacy_registry` fails while a card of `data/cards/legacy.ir.json` has no line;
//! * [`UNKNOWN`] for anything else (a card outside the cube list, an unregistered token).
//!
//! Capacity [`D`] = 1024 identities: the code's digits are one-hots of 16, 8 and 8 (index =
//! d0 * 64 + d1 * 8 + d2). A checkpoint stays valid as cards are ported (a newly supported card
//! simply starts appearing under its fixed identity); a change to the cube list itself (its
//! IR card order) or to the capacity is a new layout.
use std::sync::{Mutex, OnceLock};

use crate::db::{types, CardDb, CardDef, Kw};
use crate::ids::DefId;

/// Identity capacity (the bags' width, the token network's vocabulary).
pub const D: usize = 1024;
/// Cube cards take the identities below this (their IR position).
pub const CUBE_CAP: usize = 768;
/// A card outside the cube list or a token missing from the registry.
pub const UNKNOWN: u16 = (D - 1) as u16;
/// The token registry (one key per line; the line number is the token's index).
pub const TOKENS: &str = include_str!("tokens.txt");
/// Legacy cards take the identities from this one (the cube list's length,
/// 546: the first free identity below `CUBE_CAP`).
pub const LEGACY_BASE: usize = 546;
/// The legacy card registry (one card name per line; line k is identity
/// `LEGACY_BASE + k`; append-only).
pub const LEGACY_CARDS: &str = include_str!("legacy_cards.txt");

/// The legacy registry's names in order.
pub fn legacy_registry() -> Vec<&'static str> {
    LEGACY_CARDS.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()).collect()
}

/// Keywords in a token's key, in this order.
const KEY_KWS: [Kw; 15] = [
    Kw::Flying,
    Kw::Reach,
    Kw::FirstStrike,
    Kw::DoubleStrike,
    Kw::Deathtouch,
    Kw::Trample,
    Kw::Lifelink,
    Kw::Vigilance,
    Kw::Haste,
    Kw::Defender,
    Kw::Menace,
    Kw::Flash,
    Kw::Hexproof,
    Kw::Shroud,
    Kw::Indestructible,
];

/// A token definition's registry key, e.g. `Treasure -/- C a Treasure` or
/// `Cat 2/2 W c Cat lifelink`; a rules definition's, `Undercity rules`.
pub fn token_key(db: &CardDb, d: &CardDef) -> String {
    if d.rules {
        return format!("{} rules", d.name);
    }
    let pt = |v: Option<i32>| v.map_or("-".to_string(), |x| x.to_string());
    let colors: String = "WUBRG"
        .chars()
        .enumerate()
        .filter(|&(i, _)| d.colors & (1 << i) != 0)
        .map(|(_, c)| c)
        .collect();
    let tys: String = [(types::ARTIFACT, 'a'), (types::CREATURE, 'c'), (types::ENCHANTMENT, 'e'), (types::LAND, 'l')]
        .iter()
        .filter(|(t, _)| d.types & t != 0)
        .map(|&(_, c)| c)
        .collect();
    let mut key = format!("{} {}/{} {} {}", d.name, pt(d.power), pt(d.toughness), if colors.is_empty() { "C" } else { &colors }, tys);
    let mut subs: Vec<&str> = d.subtypes.iter().map(|&s| db.subtype_name(s)).collect();
    subs.sort_unstable();
    for s in subs {
        key.push(' ');
        key.push_str(s);
    }
    for k in KEY_KWS {
        if d.keywords.has(k) {
            key.push_str(&format!(" {k:?}").to_lowercase());
        }
    }
    key
}

/// The registry's keys in order.
pub fn registry() -> Vec<&'static str> {
    TOKENS.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()).collect()
}

/// The cube's card names in IR order (their identities).
pub fn cube_names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| crate::db::load_cube_document().cards.iter().map(|c| c.name.clone()).collect())
}

fn compute(db: &CardDb) -> Vec<u16> {
    let names = cube_names();
    assert!(names.len() <= CUBE_CAP, "the cube list ({}) exceeds the identity capacity {CUBE_CAP}", names.len());
    let mut pos: std::collections::HashMap<&str, u16> = names.iter().enumerate().map(|(i, n)| (n.as_str(), i as u16)).collect();
    let legacy = legacy_registry();
    // (A cube list of more than 546 cards would take the legacy cards' identities: a change to the
    // cube list is a new layout anyway (ENGINE.md 22.2, 54.1), which then moves LEGACY_BASE and
    // renumbers the legacy registry's cards with it.)
    assert!(
        names.len() <= LEGACY_BASE,
        "the cube list ({}) reaches the legacy identities ({LEGACY_BASE}): a new cube list is a new layout (ENGINE.md 22.2); move LEGACY_BASE above it with the layout",
        names.len()
    );
    assert!(LEGACY_BASE + legacy.len() <= CUBE_CAP, "the legacy registry ({}) exceeds the free identities", legacy.len());
    for (k, n) in legacy.into_iter().enumerate() {
        assert!(pos.insert(n, (LEGACY_BASE + k) as u16).is_none(), "legacy registry: {n} is a cube card or listed twice");
    }
    let reg: std::collections::HashMap<&str, u16> = registry().into_iter().enumerate().map(|(i, k)| (k, i as u16)).collect();
    assert!(CUBE_CAP + reg.len() < D, "the token registry ({}) exceeds the identity capacity", reg.len());
    let of = |d: &CardDef| -> u16 {
        if d.token || d.rules {
            reg.get(token_key(db, d).as_str()).map_or(UNKNOWN, |&k| CUBE_CAP as u16 + k)
        } else if d.emblem {
            d.name.strip_suffix(" emblem").and_then(|n| pos.get(n).copied()).unwrap_or(UNKNOWN)
        } else {
            pos.get(d.name.as_str()).copied().unwrap_or(UNKNOWN)
        }
    };
    (0..db.len())
        .map(|i| {
            let d = db.def(i as DefId);
            // A face (a back face, an Adventure) is its card (ENGINE.md 25), a token's face its
            // token (Sunfall's Incubator, ENGINE.md 28).
            match d.front {
                Some(f) => of(db.def(f)),
                // A face-down permanent's characteristics (CR 708.2a, ENGINE.md
                // 31): no name, so a registry key like a token's.
                None if i as DefId == db.face_down_def() => {
                    reg.get(token_key(db, d).as_str()).map_or(UNKNOWN, |&k| CUBE_CAP as u16 + k)
                }
                None => of(d),
            }
        })
        .collect()
}

/// The identity of every definition of `db` (indexed by `DefId`); computed once per card table.
pub fn identities(db: &'static CardDb) -> &'static [u16] {
    static CUBE: OnceLock<&'static [u16]> = OnceLock::new();
    if std::ptr::eq(db, CardDb::cube()) {
        return CUBE.get_or_init(|| Box::leak(compute(db).into_boxed_slice()));
    }
    static OTHERS: Mutex<Vec<(usize, &'static [u16])>> = Mutex::new(Vec::new());
    let key = db as *const CardDb as usize;
    let mut v = OTHERS.lock().unwrap();
    if let Some(&(_, ids)) = v.iter().find(|(k, _)| *k == key) {
        return ids;
    }
    let ids: &'static [u16] = Box::leak(compute(db).into_boxed_slice());
    v.push((key, ids));
    ids
}
