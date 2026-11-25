//! Sideboarding by a choice table (ENGINE.md 168.3, the spec's `"sideboard_table"`): a constructed best-of-three
//! seat whose 75 is the table's list sideboards, when it is not dealt, to the table's 60 for its context - a
//! **teacher decision** (zero-core `SearchGame::teacher_policy`, its docs/search.md 3.19): the canonical path's
//! next action (ENGINE.md 36.2 / 120.1: the entries the 60 changes, in list order, each with its new count, then
//! `PASS` - every legal 60 is reached in exactly one way), played without a search and recorded as the decision's
//! policy target, so the network's prior learns the table's choices.
//!
//! **Why a table is exact here** (ENGINE.md 168.3): the matchup model (`mtgz.az.matchup`) scores a 60 against the
//! decider's belief about the other seat's next 60, and at a sideboarding decision that belief is the same in every
//! state of one context - the other seat's 60 for the next game is dealt independently of everything played
//! (a dealt seat, 154: its restricted prior; a seat that chooses: the constructed prior, 120.2, with game 3
//! independent of game 2 by the user's decision H3 = A, 141.4) and nothing of the next game is seen yet. So the
//! model's choice is a function of the public context alone: the next game (2 or 3), whether the decider
//! sideboards first (the next game's chooser of play or draw, CR 103.1), its game wins less the other's, and
//! whether the other seat is dealt. The trainer computes the 24 contexts' choices and the spec carries them.
//!
//! The spec: `{"version": name, "list": [[card, copies], ...], "choices": [[game, first, lead, opp_dealt,
//! [copies per list entry]], ...]}` - every one of the 24 contexts once (game 2 or 3, first 0 or 1, lead -1, 0
//! or 1, opp_dealt 0 or 1), each within the list's copies, all of one size N >= 60 (CR 100.2a) - the registered main's
//! of the 75 the table is for (ENGINE.md 272; a seat's pool must have it: `fits`). Refused loudly otherwise.
use std::sync::Arc;

use serde_json::Value;

use super::matches::MatchPool;
use crate::db::CardDb;
use crate::ids::DefId;

/// A choice table (module docs).
#[derive(Debug, PartialEq, Eq)]
pub struct SideboardTable {
    /// The version's name (the trainer's, e.g. `v0040`): logged, never read by the rules.
    pub version: String,
    /// The 75: (definition, copies), sorted by definition - a constructed pool's list order
    /// (`MatchPool::constructed`).
    pub list: Vec<(DefId, u8)>,
    /// The contexts' decks, copies per `list` entry, by `context_index`.
    choices: Vec<Arc<[u8]>>,
    /// The decks' size (every context's the same): the registered main's of the 75 it is for, N >= 60 (CR 100.2a;
    /// a constructed best-of-three keeps the main's size, ENGINE.md 120, 272). Not part of `key` (the choices fix it).
    pub size: usize,
    /// A digest of the version, the list and the choices (the public state's view).
    pub key: u64,
}

/// The number of contexts: game 2 or 3, first or second, lead -1 / 0 / 1, the other seat dealt or not.
pub const CONTEXTS: usize = 24;

/// A context's index: `((game - 2) * 2 + first) * 6 + (lead + 1) * 2 + opp_dealt`.
pub fn context_index(game: u8, first: bool, lead: i8, opp_dealt: bool) -> usize {
    assert!((2..=3).contains(&game) && (-1..=1).contains(&lead), "a sideboarding context: game {game}, lead {lead}");
    ((game as usize - 2) * 2 + first as usize) * 6 + (lead + 1) as usize * 2 + opp_dealt as usize
}

fn fnv(h: &mut u64, x: u64) {
    for b in x.to_le_bytes() {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

impl SideboardTable {
    /// The spec's `"sideboard_table"` (module docs); card names looked up in `db`.
    pub fn parse(x: &Value, db: &CardDb) -> Result<SideboardTable, String> {
        Self::parse_as(x, db, "sideboard_table")
    }

    /// A table of the spec at `what` (its refusals name it): `"sideboard_table"` (ENGINE.md 168.3), a seat's own
    /// `decks.p0_table` / `decks.p1_table` or a metagame deck's `table` (ENGINE.md 240: one table per deck).
    pub fn parse_as(x: &Value, db: &CardDb, what: &str) -> Result<SideboardTable, String> {
        let err = |m: String| format!("mtg spec: {what}: {m} (ENGINE.md 168.3)");
        let o = x.as_object().ok_or_else(|| err("must be {\"version\", \"list\", \"choices\"}".into()))?;
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "version" | "list" | "choices")) {
            return Err(err(format!("unknown key {k:?} (version, list, choices)")));
        }
        let version = o.get("version").and_then(|v| v.as_str()).filter(|v| !v.is_empty()).ok_or_else(|| err("version must be a non-empty string".into()))?;
        let list = o.get("list").and_then(|v| v.as_array()).filter(|a| !a.is_empty()).ok_or_else(|| err("list must be [[card, copies], ...]".into()))?;
        // (the spec's order, then the definitions' sorted order: a constructed pool's list order)
        let mut given: Vec<(DefId, u8)> = Vec::with_capacity(list.len());
        for e in list {
            let a = e.as_array().filter(|a| a.len() == 2).ok_or_else(|| err(format!("a list entry {e} is not [card, copies]")))?;
            let name = a[0].as_str().ok_or_else(|| err(format!("a list entry {e}: the card is a name")))?;
            let d = db.id(name).ok_or_else(|| err(format!("unknown card {name:?}")))?;
            let n = a[1].as_u64().filter(|&n| (1..=31).contains(&n)).ok_or_else(|| err(format!("{name}: copies 1..=31")))? as u8;
            if given.iter().any(|g| g.0 == d) {
                return Err(err(format!("{name} listed twice")));
            }
            given.push((d, n));
        }
        let mut order: Vec<usize> = (0..given.len()).collect();
        order.sort_by_key(|&i| given[i].0);
        let sorted: Vec<(DefId, u8)> = order.iter().map(|&i| given[i]).collect();
        let ch = o.get("choices").and_then(|v| v.as_array()).ok_or_else(|| err("choices must be a list".into()))?;
        let mut choices: Vec<Option<Arc<[u8]>>> = vec![None; CONTEXTS];
        let mut size: Option<usize> = None;
        for c in ch {
            let a = c.as_array().filter(|a| a.len() == 5).ok_or_else(|| err(format!("a choice {c} is not [game, first, lead, opp_dealt, copies]")))?;
            let game = a[0].as_u64().filter(|g| (2..=3).contains(g)).ok_or_else(|| err(format!("a choice's game {}: 2 or 3", a[0])))? as u8;
            let flag = |v: &Value, what: &str| v.as_u64().filter(|&f| f <= 1).map(|f| f == 1).ok_or_else(|| err(format!("a choice's {what} {v}: 0 or 1")));
            let first = flag(&a[1], "first")?;
            let lead = a[2].as_i64().filter(|l| (-1..=1).contains(l)).ok_or_else(|| err(format!("a choice's lead {}: -1, 0 or 1", a[2])))? as i8;
            let opp_dealt = flag(&a[3], "opp_dealt")?;
            let cnt = a[4].as_array().filter(|v| v.len() == given.len()).ok_or_else(|| err(format!("a choice's copies: one per list entry ({})", given.len())))?;
            let mut deck = vec![0u8; given.len()];
            for (j, &i) in order.iter().enumerate() {
                let n = cnt[i].as_u64().filter(|&n| n <= given[i].1 as u64).ok_or_else(|| err(format!("a choice's copies of {}: at most {}", db.def(given[i].0).name, given[i].1)))?;
                deck[j] = n as u8;
            }
            // (ENGINE.md 272: every choice the same size, the registered main's - at least 60, CR 100.2a; `fits`
            // checks it is the pool's)
            let total: usize = deck.iter().map(|&n| n as usize).sum();
            let min = super::constructed::MIN_DECK;
            if total < min {
                return Err(err(format!("the choice of game {game}, first {first}, lead {lead}, opp_dealt {opp_dealt} has {total} cards (CR 100.2a: at least {min})")));
            }
            match size {
                None => size = Some(total),
                Some(n) if n != total => {
                    return Err(err(format!(
                        "the choice of game {game}, first {first}, lead {lead}, opp_dealt {opp_dealt} has {total} cards, another choice {n} (every deck the registered main's size)"
                    )))
                }
                _ => {}
            }
            let k = context_index(game, first, lead, opp_dealt);
            if choices[k].is_some() {
                return Err(err(format!("the context game {game}, first {first}, lead {lead}, opp_dealt {opp_dealt} twice")));
            }
            choices[k] = Some(Arc::from(deck));
        }
        if let Some(k) = choices.iter().position(|c| c.is_none()) {
            return Err(err(format!("{} of the {CONTEXTS} contexts given (the first missing: index {k})", choices.iter().filter(|c| c.is_some()).count())));
        }
        let choices: Vec<Arc<[u8]>> = choices.into_iter().map(|c| c.unwrap()).collect();
        let mut key: u64 = 0xcbf29ce484222325;
        for b in version.bytes() {
            fnv(&mut key, b as u64);
        }
        for &(d, n) in &sorted {
            fnv(&mut key, d as u64 | (n as u64) << 32);
        }
        for c in &choices {
            for &n in c.iter() {
                fnv(&mut key, n as u64);
            }
        }
        Ok(SideboardTable { version: version.to_string(), list: sorted, choices, key, size: size.expect("24 contexts given") })
    }

    /// Is `pool` the table's 75 (a constructed pool: the same list of definitions and copies) with its decks' size (the
    /// registered main's, ENGINE.md 272: one list of 76 can be a 61 and 15 or a 60 and 16)?
    pub fn fits(&self, pool: &MatchPool) -> bool {
        pool.n_cards == pool.list.len() && pool.list == self.list && pool.min_deck == self.size && pool.max_deck == self.size
    }

    /// The deck of a context (the registered main's size), copies per list entry.
    pub fn choice(&self, game: u8, first: bool, lead: i8, opp_dealt: bool) -> &Arc<[u8]> {
        &self.choices[context_index(game, first, lead, opp_dealt)]
    }

    /// The canonical path's next action from a sideboarding state (ENGINE.md 36.2): the decider's deck being built
    /// `counts` (entries before `from` already final), a pending count question of entry `asking`, toward `target`.
    /// An entry of one copy toggles; any other is named, then its new count. `PASS` once every entry is the target's.
    /// Panics when the entries already passed differ from the target (the path was left: never for a seat that
    /// follows the table from its first decision).
    pub fn next_action(pool: &MatchPool, counts: &[u8], from: usize, asking: Option<u8>, target: &[u8]) -> u16 {
        use crate::act;
        assert_eq!(&counts[..from], &target[..from], "the sideboarding left the table's path (ENGINE.md 168.3)");
        if let Some(k) = asking {
            return act::NUMBER + target[k as usize] as u16;
        }
        match (from..pool.list.len()).find(|&k| counts[k] != target[k]) {
            Some(k) => act::LIBRARY + k as u16,
            None => act::PASS,
        }
    }
}
