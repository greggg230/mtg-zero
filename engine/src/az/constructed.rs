//! Constructed decks (ENGINE.md 54): the deck rules of constructed play (CR
//! 100.2a, 100.4a) and the checked-in decklists of `data/decks/`.
//!
//! A constructed game of `mtg` (`"constructed": true`, `MtgSpec`) plays fixed
//! decklists with the card table of constructed play (`CardDb::constructed`:
//! the cube's cards and the legacy ones). The limited rules (a deck built from
//! a pool is exactly 40 cards, ENGINE.md 42) stay what they were: nothing here
//! touches the cube's generators, pools, drafts or sideboarding.

use crate::db::{supertypes, types, CardDb, CardDef};
use crate::ids::DefId;
use serde_json::Value;

/// CR 100.2a: a constructed deck has at least 60 cards.
pub const MIN_DECK: usize = 60;
/// CR 100.2a: at most four of a card (by English name) other than basic lands,
/// over the deck and the sideboard together (100.4a).
pub const MAX_COPIES: usize = 4;
/// CR 100.4a: a constructed sideboard has at most 15 cards.
pub const MAX_SIDEBOARD: usize = 15;
/// The largest deck the engine can play: every card of a deck may end up in
/// one zone, and a hand, a searched or looked-at library list and a graveyard
/// each have `act::HAND_N`, `LIBRARY_N` and `GRAVEYARD_N` action slots (ENGINE.md
/// 11): 64. (CR 100.2a has no maximum; a larger deck is refused, never played
/// into an out-of-range slot.)
pub const MAX_DECK: usize = {
    let (h, l, g) = (crate::decision::act::HAND_N, crate::decision::act::LIBRARY_N, crate::decision::act::GRAVEYARD_N);
    let m = if h < l { h } else { l };
    (if m < g { m } else { g }) as usize
};

/// The pinned deck of the Delver mirror (ENGINE.md 54): "Izzet Delver" by
/// i_b_TRUE, Legacy Challenge 32, 2026-10-23, 1st place (8-1), MTGO;
/// <https://www.mtggoldfish.com/deck/8016541>.
pub const IZZET_DELVER: &str = include_str!("../../../data/decks/izzet_delver_8016541.json");
/// Its MTGO export, verbatim (`/deck/download/8016541`).
pub const IZZET_DELVER_EXPORT: &str = include_str!("../../../data/decks/izzet_delver_8016541.txt");

/// A basic land card (CR 205.4c, 100.2a: any number in a deck).
pub fn is_basic_land(d: &CardDef) -> bool {
    d.supertypes & supertypes::BASIC != 0 && d.types & types::LAND != 0
}

/// CR 100.2a, 100.4a: at least 60 cards in the deck, at most 15 in the
/// sideboard, at most four of each card but basic lands over both together;
/// only cards (no tokens); and at most [`MAX_DECK`] (the engine's limit).
pub fn check(db: &CardDb, deck: &[DefId], sideboard: &[DefId]) -> Result<(), String> {
    if deck.len() < MIN_DECK {
        return Err(format!("{} cards in the deck (CR 100.2a: at least {MIN_DECK})", deck.len()));
    }
    if deck.len() > MAX_DECK {
        return Err(format!(
            "{} cards in the deck: the engine plays at most {MAX_DECK} (the action slots of a hand, a library list, a graveyard; ENGINE.md 11, 54.4)",
            deck.len()
        ));
    }
    if sideboard.len() > MAX_SIDEBOARD {
        return Err(format!("{} cards in the sideboard (CR 100.4a: at most {MAX_SIDEBOARD})", sideboard.len()));
    }
    let mut all: Vec<DefId> = deck.iter().chain(sideboard).copied().collect();
    if let Some(&t) = all.iter().find(|&&d| d as usize >= db.len() || db.def(d).token || db.is_face(d)) {
        return Err(format!("{t} is not a card"));
    }
    all.sort_unstable();
    for w in all.chunk_by(|a, b| a == b) {
        let d = db.def(w[0]);
        if w.len() > MAX_COPIES && !is_basic_land(d) {
            return Err(format!("{} copies of {} (CR 100.2a: at most {MAX_COPIES} with the sideboard, 100.4a)", w.len(), d.name));
        }
    }
    Ok(())
}

/// A decklist of `data/decks/` (JSON: its source, event, date and player;
/// `main` and `sideboard` as [count, card name] with the card table's names).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decklist {
    pub name: String,
    pub player: String,
    pub event: String,
    pub date: String,
    pub source: String,
    pub main: Vec<(u32, String)>,
    pub sideboard: Vec<(u32, String)>,
    /// MTGO export names that differ from the card table's (a double-faced
    /// card by its front face, a split card with one slash).
    pub export_names: Vec<(String, String)>,
}

impl Decklist {
    pub fn parse(json: &str) -> Result<Decklist, String> {
        let v: Value = serde_json::from_str(json).map_err(|e| format!("decklist: {e}"))?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string).ok_or_else(|| format!("decklist: no {k}"));
        let list = |k: &str| -> Result<Vec<(u32, String)>, String> {
            v.get(k)
                .and_then(|x| x.as_array())
                .ok_or_else(|| format!("decklist: no {k}"))?
                .iter()
                .map(|e| {
                    let n = e.get(0).and_then(|x| x.as_u64()).filter(|&n| n > 0);
                    let name = e.get(1).and_then(|x| x.as_str());
                    match (n, name) {
                        (Some(n), Some(name)) => Ok((n as u32, name.to_string())),
                        _ => Err(format!("decklist: bad {k} entry {e}")),
                    }
                })
                .collect()
        };
        let export_names = match v.get("export_names").and_then(|x| x.as_object()) {
            Some(m) => m.iter().map(|(a, b)| (a.clone(), b.as_str().unwrap_or_default().to_string())).collect(),
            None => Vec::new(),
        };
        Ok(Decklist {
            name: s("name")?,
            player: s("player")?,
            event: s("event")?,
            date: s("date")?,
            source: s("source")?,
            main: list("main")?,
            sideboard: list("sideboard")?,
            export_names,
        })
    }

    /// The pinned Izzet Delver 75.
    pub fn izzet_delver() -> Decklist {
        Decklist::parse(IZZET_DELVER).expect("data/decks/izzet_delver_8016541.json")
    }

    /// An MTGO export ("4 Brainstorm" lines; a blank line, then the
    /// sideboard) as (main, sideboard) with the card table's names.
    pub fn parse_export(&self, text: &str) -> Result<(Vec<(u32, String)>, Vec<(u32, String)>), String> {
        let (mut main, mut side) = (Vec::new(), Vec::new());
        let mut in_side = false;
        for line in text.lines().map(|l| l.trim_end_matches('\r').trim()) {
            if line.is_empty() {
                in_side = !main.is_empty();
                continue;
            }
            let (n, name) = line.split_once(' ').ok_or_else(|| format!("export: bad line {line:?}"))?;
            let n: u32 = n.parse().map_err(|_| format!("export: bad count in {line:?}"))?;
            let name = self.export_names.iter().find(|(a, _)| a == name).map_or(name.to_string(), |(_, b)| b.clone());
            if in_side { side.push((n, name)) } else { main.push((n, name)) }
        }
        Ok((main, side))
    }

    /// The cards of a list, expanded, by the card table's names (an error
    /// names an unsupported or unknown card).
    pub fn ids(db: &CardDb, list: &[(u32, String)]) -> Result<Vec<DefId>, String> {
        let mut out = Vec::new();
        for (n, name) in list {
            let id = db.expect_id(name)?;
            out.extend(std::iter::repeat_n(id, *n as usize));
        }
        Ok(out)
    }

    /// The maindeck's card names, expanded (a spec's `decks.p0` list).
    pub fn main_names(&self) -> Vec<String> {
        self.main.iter().flat_map(|(n, name)| std::iter::repeat_n(name.clone(), *n as usize)).collect()
    }

    /// The sideboard's cards a card table doesn't support (by name).
    pub fn unsupported_sideboard(&self, db: &CardDb) -> Vec<String> {
        self.sideboard.iter().filter(|(_, n)| db.id(n).is_none()).map(|(_, n)| n.clone()).collect()
    }
}
