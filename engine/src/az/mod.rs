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
//! * `encode`: `encode.rs` (layout v6, 44,212 floats, no hidden information).
//! * Decks: `decks.rs` (random two-colour decks per game, or the fixed
//!   evaluation set, or given lists).
//! * Bots: `random` (zero-core) and `heuristic` (`bots.rs`).
//!
//! The game `team_draft` (DESIGN.md 2.3, ENGINE.md 41, 47): `draft.rs` (the
//! event, deckbuilding, the deferred terminal), `draft_encode.rs` (layout
//! `draft_v2`), `draft_bots.rs` (`consensus`, heuristic-played matches).

pub mod belief;
pub mod bots;
pub mod constructed;
pub mod decks;
pub mod draft;
pub mod draft_bots;
pub mod draft_encode;
pub mod encode;
pub mod eoe;
pub mod game_log;
pub mod hidden_decks;
pub mod identity;
pub mod lab;
pub mod matches;
pub mod pools;
pub mod sample;
pub mod sealed;
pub mod view;

use crate::db::CardDb;
use crate::game::{Game, GameSpec};
use crate::ids::DefId;
use crate::rng::Rng;
use serde_json::Value;
use zero_search::game::{Bot, Next, SearchGame, MAX_SEATS};

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
    /// A match of a team draft (DESIGN.md 2.3.6, ENGINE.md 45): each
    /// player's drafted deck and sideboard (the rest of its pool), its real
    /// first picks (hidden from the other player in best-of-three, ENGINE.md
    /// 36.11) and the cards it knows are elsewhere (its team saw them in the
    /// draft: they narrow its prior of the opponent's unseen first picks).
    Drafted(Box<[Seat; 2]>),
}

/// A share in (0, 1] of a spec (`MtgSpec::deck_tilt_ess`), compared by its bits (the spec is `Eq`).
#[derive(Clone, Copy, Debug)]
pub struct Share(pub f64);

impl PartialEq for Share {
    fn eq(&self, o: &Share) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}

impl Eq for Share {}

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
///
/// Drafted matches (M6 part 3, DESIGN.md 2.3.6, ENGINE.md 45): `decks` may
/// also hold `"p0_first_picks"` / `"p1_first_picks"` (the player's real first
/// picks, cards of its pool: hidden from the other player in best-of-three
/// as generated pools' are, ENGINE.md 36.11) and `"p0_elsewhere"` /
/// `"p1_elsewhere"` (the cards the player knows are elsewhere: its team saw
/// them in the draft and they are not in this opponent's pool; they leave
/// its prior of the opponent's unseen first picks); `"seed"` (top level) is
/// the match's seed (default: the driver's game seed), so a spec built from
/// a `team_draft` payload plays exactly the event's match. Without these
/// keys every spec is what it was (the behaviour digests are unchanged).
///
/// Constructed (ENGINE.md 54): `"constructed": true` plays fixed 60-card
/// decklists (`decks` lists, CR 100.2a: at least 60 cards, at most four of a
/// card but basic lands, a sideboard of at most 15, 100.4a, the four-card
/// limit over deck and sideboard together) with the card table of constructed
/// play (`CardDb::constructed`: the cube's cards and the legacy ones, Murktide
/// Regent, Prismari Charm). Best-of-one only for now (constructed
/// sideboarding between games is not built: `"bo3"` is an error), and never
/// random, evaluation or drafted decks (those are limited's).
///
/// Hidden decklists (ENGINE.md 102): `"hidden_decklists": true` (default false; best-of-one,
/// not with constructed, drafted or `start`) hides each player's deck from the other entirely
/// (EoE sealed: nobody sees the opponent's pool): `determinize` deals the opponent's whole unseen
/// deck from a prior (`hidden_decks.rs`: on this branch the table's `TablePrior`) conditioned on
/// what the viewer has seen, and no encoding shows what of their decklist they have not shown.
/// Without it every spec is what it was.
///
/// One episode per game (ENGINE.md 121): `"game_episodes": true` (best-of-three
/// only; default false) makes each game of the match its own zero-core
/// episode (`SearchGame::chained`): a game's end is terminal for the search
/// and for the labels, `next_episode` continues the match. Without it every
/// spec is what it was.
///
/// `"log": true` keeps each game's log (`GameSpec::log`) for the spectator view
/// (`view.rs`, the replay recorder); it changes nothing else.
///
/// The position lab (ENGINE.md 76): `"start"` starts every game at a spot of another game of
/// this spec (its seed and actions), the hidden cards dealt again, the decider's first moves
/// forced (`lab.rs`); best-of-one only, not with `"seed"`. Without it every game is what it was.
///
/// Sealed (ENGINE.md 101, `sealed/`): `"sealed": {"set": "eoe", "collation": "arena" | "paper",
/// "boosters": 6, "builder": {...}, "p0": ..., "p1": ...}` plays best-of-one on the Edge of
/// Eternities card table (`CardDb::eoe`): each game generates a pool per player from its seed
/// (the collation's six Play Boosters), builds both decks with the randomised builder (40 cards,
/// the rest of the pool the sideboard) and plays them with open decklists; `"decks": "eval"` takes
/// the pools and builds of evaluation pair `split64(seed) % eval_decks` of `eval_seed` instead.
/// `p0` / `p1` give a seat a pool (`{"pool": [names]}`, built every game) or a build (`{"deck":
/// [...], "sideboard": [...]}`, played as it is; with `"order": "given"` its library before the
/// setup shuffle is the list in that order, the paired deck comparison's slots, ENGINE.md 104).
/// Best-of-one only; not with `constructed`, `pools`, `lands` (the builder has its own) or
/// decklists. Without the key every spec is what it was.
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
    pub seed: Option<u64>,
    /// Constructed decklists and their card table (ENGINE.md 54).
    pub constructed: bool,
    /// Keep the games' log (`GameSpec::log`): the replay recorder's games, for the viewer's
    /// play-by-play (`view.rs`). Off in self-play and evaluation (it costs a clone per log line).
    pub log: bool,
    /// The position lab's start (ENGINE.md 76, `lab.rs`): games begin at this spot.
    pub start: Option<std::sync::Arc<lab::Start>>,
    /// Hidden decklists (ENGINE.md 102).
    pub hidden_decklists: bool,
    /// Hidden decklists' K-sample deal (ENGINE.md 102.8): the prior's K decks per request, one
    /// picked per deal (`"deal_samples"`, default 32; 0 = sample the prior at every deal).
    pub deal_samples: usize,
    /// The deck tilt's effective-sample-size floor, a share of the K decks in (0, 1] (ENGINE.md 106.1;
    /// `"deck_tilt_ess"`, default `hidden_decks::DECK_TILT_ESS` 0.5). Read only by a deck-tilted deal.
    pub deck_tilt_ess: Share,
    /// Sealed play (ENGINE.md 101): the pools, the builder and the seats; None for every other spec.
    pub sealed: Option<sealed::Shared>,
    /// One zero-core episode per game of a best-of-three match (ENGINE.md 121).
    pub game_episodes: bool,
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
            seed: None,
            constructed: false,
            log: false,
            start: None,
            hidden_decklists: false,
            deal_samples: 32,
            deck_tilt_ess: Share(hidden_decks::DECK_TILT_ESS),
            sealed: None,
            game_episodes: false,
        }
    }
}

/// The keys of a spec's `decks` object: the lists, then a drafted match's
/// knowledge (any of the last four makes the match a drafted one).
const DECKS_KEYS: [&str; 8] =
    ["p0", "p1", "p0_sideboard", "p1_sideboard", "p0_first_picks", "p1_first_picks", "p0_elsewhere", "p1_elsewhere"];

/// A drafted match's seats are consistent (ENGINE.md 45): each player's first
/// picks are distinct cards of its pool (deck and sideboard; no basic land),
/// and what it knows is elsewhere is in no place of the other player's pool
/// (else it would know a first pick of theirs, or think a card of their pool
/// is not theirs: the real world would be outside its own information set).
fn check_drafted(db: &CardDb, seats: &[Seat; 2]) -> Result<(), String> {
    let basics = pools::basic_ids(db);
    for (p, s) in seats.iter().enumerate() {
        let mut pool: Vec<DefId> = s.deck.iter().chain(&s.sideboard).copied().filter(|d| !basics.contains(d)).collect();
        pool.sort_unstable();
        for w in s.first_picks.windows(2) {
            if w[0] == w[1] {
                return Err(format!("mtg spec: p{p}_first_picks names {} twice", db.def(w[0]).name));
            }
        }
        for &d in &s.first_picks {
            if pool.binary_search(&d).is_err() {
                return Err(format!("mtg spec: p{p}'s first pick {} is not in its pool (deck or sideboard)", db.def(d).name));
            }
        }
        if let Some(&d) = s.elsewhere.iter().find(|d| basics.contains(d)) {
            return Err(format!("mtg spec: p{p}_elsewhere names a basic land ({})", db.def(d).name));
        }
        let o = &seats[1 - p];
        if let Some(&d) = s.elsewhere.iter().find(|d| o.deck.contains(d) || o.sideboard.contains(d)) {
            let what = if o.first_picks.contains(&d) { "a first pick" } else { "a card" };
            return Err(format!("mtg spec: p{p}_elsewhere names {} ({what} of p{}'s pool)", db.def(d).name, 1 - p));
        }
    }
    Ok(())
}

impl MtgSpec {
    pub fn parse(json: &str) -> Result<MtgSpec, String> {
        let mut s = MtgSpec::default();
        if json.trim().is_empty() {
            return Ok(s);
        }
        let v: Value = serde_json::from_str(json).map_err(|e| format!("mtg spec: {e}"))?;
        let obj = v.as_object().ok_or("mtg spec: not a JSON object")?;
        // (Read first: it picks the card table the names are looked up in.)
        if let Some(x) = obj.get("constructed") {
            s.constructed = x.as_bool().ok_or("mtg spec: constructed must be a boolean")?;
        }
        if let Some(x) = obj.get("sealed") {
            s.sealed = Some(std::sync::Arc::new(sealed::SealedSpec::parse(x)?));
            // Sealed hides the decklists (ENGINE.md 102.6: nobody sees the opponent's pool), unless
            // the spec says `"hidden_decklists": false` (open decklists, ENGINE.md 101.3's game).
            s.hidden_decklists = true;
        }
        let db = s.db();
        let mut pools = None;
        for (k, x) in obj {
            let int = || x.as_u64().ok_or_else(|| format!("mtg spec: {k} must be a non-negative integer"));
            match k.as_str() {
                "sealed" => {}
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
                "seed" => s.seed = Some(int()?),
                "constructed" => {}
                "log" => s.log = x.as_bool().ok_or("mtg spec: log must be a boolean")?,
                "hidden_decklists" => s.hidden_decklists = x.as_bool().ok_or("mtg spec: hidden_decklists must be a boolean")?,
                "game_episodes" => s.game_episodes = x.as_bool().ok_or("mtg spec: game_episodes must be a boolean")?,
                "deal_samples" => s.deal_samples = int()? as usize,
                "deck_tilt_ess" => {
                    let e = x.as_f64().filter(|e| *e > 0.0 && *e <= 1.0).ok_or("mtg spec: deck_tilt_ess must be a number in (0, 1] (a share of the K decks)")?;
                    s.deck_tilt_ess = Share(e);
                }
                "start" => s.start = Some(std::sync::Arc::new(lab::Start::parse(x, db)?)),
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
                                if !DECKS_KEYS.contains(&key.as_str()) {
                                    return Err(format!("mtg spec: unknown decks key {key:?}"));
                                }
                            }
                            let side = |key: &str| if o.contains_key(key) { list(key) } else { Ok(Vec::new()) };
                            let (s0, s1) = (side("p0_sideboard")?, side("p1_sideboard")?);
                            let drafted = DECKS_KEYS[4..].iter().any(|k| o.contains_key(*k));
                            if drafted {
                                let seat = |p: usize, deck: Vec<DefId>, sideboard: Vec<DefId>| -> Result<Seat, String> {
                                    let mut first_picks = side(&format!("p{p}_first_picks"))?;
                                    first_picks.sort_unstable();
                                    let mut elsewhere = side(&format!("p{p}_elsewhere"))?;
                                    elsewhere.sort_unstable();
                                    elsewhere.dedup();
                                    Ok(Seat { deck, sideboard, first_picks, elsewhere })
                                };
                                let seats = [seat(0, list("p0")?, s0)?, seat(1, list("p1")?, s1)?];
                                check_drafted(db, &seats)?;
                                DeckMode::Drafted(Box::new(seats))
                            } else if s0.is_empty() && s1.is_empty() {
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
        if s.start.is_some() && (s.seed.is_some() || s.format != Format::Bo1) {
            return Err("mtg spec: start is for best-of-one games without a match seed".into());
        }
        s.pools = pools.unwrap_or(s.format == Format::Bo3);
        if s.game_episodes && s.format != Format::Bo3 {
            return Err("mtg spec: game_episodes is for best-of-three (\"format\": \"bo3\"): a best-of-one episode is its game already (ENGINE.md 121)".into());
        }
        if s.hidden_decklists {
            // (ENGINE.md 102: best-of-three hides decks within known pools, 36.11; constructed
            // and drafted decks need priors of their own; a lab start deals its own spot.)
            if s.format != Format::Bo1 {
                return Err("mtg spec: hidden_decklists is for best-of-one (best-of-three hides decks within pools, ENGINE.md 36.11)".into());
            }
            if s.constructed || matches!(s.decks, DeckMode::Drafted(_)) || s.start.is_some() {
                return Err("mtg spec: hidden_decklists needs limited decks of the spec (not constructed, drafted or a lab start: no prior for them yet, ENGINE.md 102)".into());
            }
        }
        if s.sealed.is_some() {
            if s.format != Format::Bo1 {
                return Err("mtg spec: sealed best-of-three (sideboarding, hidden decklists) is not built yet: format must be \"bo1\"".into());
            }
            if s.constructed {
                return Err("mtg spec: sealed and constructed are different formats".into());
            }
            if s.pools {
                return Err("mtg spec: sealed pools come from the sealed key, not pools".into());
            }
            if obj.contains_key("lands") {
                return Err("mtg spec: sealed decks' lands are the builder's (sealed.builder.lands)".into());
            }
            if !matches!(s.decks, DeckMode::Random | DeckMode::Eval) {
                return Err("mtg spec: sealed seats are given as sealed.p0 / sealed.p1, not decks lists".into());
            }
            // (ENGINE.md 107.2: a seat's chosen deck is hidden from the other player)
            if s.sealed.as_ref().is_some_and(|x| x.deck_choice.is_some()) && !s.hidden_decklists {
                return Err("mtg spec: sealed.deck_choice needs hidden decklists (a seat's choice is hidden from the other player)".into());
            }
            if s.sealed.as_ref().is_some_and(|x| x.deck_choice.is_some()) && s.start.is_some() {
                return Err("mtg spec: sealed.deck_choice is the match's start, not with a lab start".into());
            }
        }
        if s.constructed {
            if s.format != Format::Bo1 {
                return Err("mtg spec: constructed best-of-three (sideboarding to 60+ / 15) is not built yet: format must be \"bo1\"".into());
            }
            if s.pools {
                return Err("mtg spec: constructed decks don't come from pools".into());
            }
            match &s.decks {
                DeckMode::Lists(a, b) => {
                    for (p, d) in [a, b].into_iter().enumerate() {
                        constructed::check(db, d, &[]).map_err(|e| format!("mtg spec: p{p}'s constructed deck: {e}"))?;
                    }
                }
                DeckMode::ListsWithSideboards(p) => {
                    for (i, (d, sb)) in p.iter().enumerate() {
                        constructed::check(db, d, sb).map_err(|e| format!("mtg spec: p{i}'s constructed deck: {e}"))?;
                    }
                }
                _ => return Err("mtg spec: constructed needs decklists (decks: {\"p0\": [...], \"p1\": [...]})".into()),
            }
        }
        // The engine's deck limit for every given list (ENGINE.md 58.3; the
        // random, evaluation and drafted decks have 40 cards).
        let lists: Vec<(&[DefId], &[DefId])> = match &s.decks {
            DeckMode::Lists(a, b) => vec![(a, &[]), (b, &[])],
            DeckMode::ListsWithSideboards(p) => p.iter().map(|(d, sb)| (&d[..], &sb[..])).collect(),
            DeckMode::Drafted(seats) => seats.iter().map(|x| (&x.deck[..], &x.sideboard[..])).collect(),
            _ => Vec::new(),
        };
        for (p, (d, sb)) in lists.into_iter().enumerate() {
            constructed::check_size(db, d, sb).map_err(|e| format!("mtg spec: p{p}'s deck: {e}"))?;
        }
        Ok(s)
    }

    /// The zero-core game this spec's games are (ENGINE.md 105): a sealed spec plays the EoE
    /// table, whose encodings carry the EoE identity space, so it is `mtg_eoe`'s (`eoe.rs`);
    /// every other spec is `mtg`'s.
    pub fn game(&self) -> &'static str {
        if self.sealed.is_some() {
            "mtg_eoe"
        } else {
            "mtg"
        }
    }

    /// An error unless this spec is game `game`'s (`game`): a network of one identity space is
    /// never fed another's encodings through the wrong game's name.
    pub fn check_game(&self, game: &str) -> Result<(), String> {
        let mine = self.game();
        if mine == game {
            return Ok(());
        }
        Err(if mine == "mtg_eoe" {
            format!("{game} spec: a sealed spec (Edge of Eternities) is game mtg_eoe: its encodings carry the EoE identities (ENGINE.md 105)")
        } else {
            format!("{game} spec: game {game} plays Edge of Eternities sealed (the \"sealed\" key); this spec is game {mine}'s (ENGINE.md 105)")
        })
    }

    /// The card table this spec's games use: the cube's, or with
    /// `constructed` the cube's and the legacy cards (ENGINE.md 54).
    pub fn db(&self) -> &'static CardDb {
        if self.sealed.is_some() {
            sealed::db()
        } else if self.constructed {
            CardDb::constructed()
        } else {
            CardDb::cube()
        }
    }

    /// The seed whose pools and builds a sealed game with seed `seed` plays: `seed`, or with
    /// `"decks": "eval"` evaluation pair `split64(seed) % eval_decks`'s (ENGINE.md 101.3).
    pub fn sealed_seed(&self, seed: u64) -> u64 {
        match self.decks {
            DeckMode::Eval => {
                let i = (zero_search::rng::split64(seed) % self.eval_decks as u64) as u32;
                sealed::eval_game_seed(self.eval_seed, i)
            }
            _ => seed,
        }
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
    /// this seed (ENGINE.md 36.4). Panics where [`try_seats`](Self::try_seats)
    /// fails (a sealed pool holding a card the engine doesn't support).
    pub fn seats(&self, seed: u64) -> [Seat; 2] {
        self.try_seats(seed).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`seats`](Self::seats), or why there are none: a sealed game's
    /// generated pool can hold a card the engine doesn't support (ENGINE.md
    /// 101.3: a loud failure, never a dropped card). Every other spec has seats.
    pub fn try_seats(&self, seed: u64) -> Result<[Seat; 2], String> {
        if let Some(s) = &self.sealed {
            return s.seats(self.sealed_seed(seed)).map_err(|e| format!("mtg spec: sealed game {seed}: {e}"));
        }
        Ok(self.cube_seats(seed))
    }

    fn cube_seats(&self, seed: u64) -> [Seat; 2] {
        let db = CardDb::cube();
        let built = |p: pools::Pool| -> Seat {
            let (deck, sideboard) = pools::build_deck(db, &p.cards, &self.gen);
            Seat { deck, sideboard, first_picks: p.first_picks, elsewhere: Vec::new() }
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
        if let DeckMode::Drafted(seats) = &self.decks {
            return (**seats).clone();
        }
        let seat = |(deck, sideboard): (Vec<DefId>, Vec<DefId>)| Seat { deck, sideboard, first_picks: Vec::new(), elsewhere: Vec::new() };
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
            DeckMode::Drafted(_) => unreachable!("returned above"),
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
    /// A position-lab game's forced line and follow rule (ENGINE.md 76, `lab.rs`); None in every
    /// other game.
    pub lab: Option<Box<lab::Lab>>,
}

impl MtgAz {
    pub fn new(spec: &MtgSpec, seed: u64) -> Result<MtgAz, String> {
        // (A sealed game: each seat's deck and how it was made, ENGINE.md 107; with a deck choice the
        // choosing seats' candidates, the game started with their first candidate until they choose.)
        let (seats, how, choice) = match spec.sealed.as_deref() {
            Some(sl) => Self::sealed_seats(sl, spec.sealed_seed(seed)).map_err(|e| format!("mtg spec: sealed game {seed}: {e}"))?,
            None => (spec.try_seats(seed)?, [None, None], None),
        };
        // Best-of-three: the decks are hidden within the pools (ENGINE.md
        // 36.11); best-of-one keeps open decklists (M5a-M8a's game).
        let gspec = GameSpec {
            max_turns: spec.max_turns,
            mulligans: spec.mulligans,
            chooser: spec.chooser,
            hidden_decks: spec.format == Format::Bo3 || spec.hidden_decklists,
            hidden_decklists: spec.hidden_decklists,
            log: spec.log,
            ..GameSpec::default()
        };
        // (The prior of each deck, ENGINE.md 102: a sealed spec's seats the sealed prior of its
        // collation, boosters and builder; any other spec the stub over its table.)
        let priors: Option<std::sync::Arc<dyn hidden_decks::DeckPrior>> = spec.hidden_decklists.then(|| match &spec.sealed {
            Some(sl) => std::sync::Arc::new(hidden_decks::SealedPrior::of_spec(sl)) as std::sync::Arc<dyn hidden_decks::DeckPrior>,
            None => hidden_decks::TablePrior::of(spec.db()) as std::sync::Arc<dyn hidden_decks::DeckPrior>,
        });
        let mut az = MtgAz::from_seats(spec.db(), gspec, spec.format, &seats, seed, [priors.clone(), priors])?;
        az.m.deal.k = spec.deal_samples;
        az.m.deal.ess = spec.deck_tilt_ess.0;
        az.m.how = how;
        az.m.choice = choice;
        az.m.game_episodes = spec.game_episodes;
        Ok(az)
    }

    /// A sealed game's seats (the game whose pools come from `seed`), how each deck was made, and the
    /// deck choice (ENGINE.md 107.2) if a seat chooses: a choosing seat's seat is its candidate 0 until
    /// it chooses; an exploring one plays its explore pick; a seat without candidates (a given deck, or
    /// no deck choice) the spec's seat (`SealedSpec::seat`, 107.1's modes).
    #[allow(clippy::type_complexity)]
    fn sealed_seats(sl: &sealed::SealedSpec, seed: u64) -> Result<([Seat; 2], [Option<matches::SeatHow>; 2], Option<Box<matches::DeckChoice>>), String> {
        use matches::{Candidate, DeckChoice, SeatHow};
        let mut seats: Vec<Seat> = Vec::with_capacity(2);
        let mut how = [None, None];
        let mut choosers: Vec<crate::ids::PlayerId> = Vec::new();
        let mut cands: [Option<std::sync::Arc<[Candidate]>>; 2] = [None, None];
        for p in 0..2u8 {
            let seat = |deck: &[DefId], sideboard: &[DefId]| Seat { deck: deck.to_vec(), sideboard: sideboard.to_vec(), first_picks: Vec::new(), elsewhere: Vec::new() };
            match (sl.deck_choice, sl.candidates(seed, p)?) {
                (Some(dc), Some((builds, ps))) => {
                    let cs: Vec<Candidate> = builds
                        .into_iter()
                        .enumerate()
                        .map(|(j, b)| {
                            let mode = if j == 0 { sealed::Mode::Best } else { sealed::Mode::Explore };
                            let how = SeatHow { mode, candidate: Some(j as u8), pool_seed: ps, colors: b.colors, splash: b.splash };
                            Candidate { deck: b.deck, sideboard: b.sideboard, how }
                        })
                        .collect();
                    let pick = sealed::explore_pick(seed, p, dc.explore.0, dc.k);
                    let c = &cs[pick.unwrap_or(0) as usize];
                    seats.push(seat(&c.deck, &c.sideboard));
                    match pick {
                        Some(_) => how[p as usize] = Some(SeatHow { mode: sealed::Mode::Explore, ..c.how }),
                        None => {
                            choosers.push(p);
                            cands[p as usize] = Some(std::sync::Arc::from(cs));
                        }
                    }
                }
                _ => {
                    let (s, info) = sl.seat(seed, p)?;
                    let (colors, splash) = info.build.as_ref().map_or((0, 0), |b| (b.colors, b.splash));
                    how[p as usize] = Some(SeatHow { mode: info.mode, candidate: None, pool_seed: info.pool_seed, colors, splash });
                    seats.push(s);
                }
            }
        }
        let choice = (!choosers.is_empty()).then(|| Box::new(DeckChoice::new(&choosers, cands)));
        let s1 = seats.pop().expect("two seats");
        let s0 = seats.pop().expect("two seats");
        Ok(([s0, s1], how, choice))
    }

    /// `determinize` during the deck choice (ENGINE.md 107.2): the other seat's deck dealt from its prior
    /// (nothing of it is known: hidden decklists), as the match's pool and deck of theirs and their cards
    /// of the game in waiting; in the deciding viewer's world the other seat has chosen (its deck the
    /// dealt one, its candidates gone: the choice ends with the viewer's); for another viewer the
    /// decider's candidates are K copies of its dealt deck (whatever it chooses, its deck is unknown).
    fn determinize_deck_choice(&mut self, viewer: crate::ids::PlayerId, rng: &mut Rng) {
        let o = crate::ids::opp(viewer);
        self.sample_hidden_deck(viewer, rng, None, None);
        self.m.how[o as usize] = None;
        let dealt = matches::Candidate {
            deck: self.m.deck(o),
            sideboard: self.m.pools[o as usize].sideboard(&self.m.decks[o as usize]),
            how: matches::SeatHow { mode: sealed::Mode::Explore, candidate: None, pool_seed: None, colors: 0, splash: 0 },
        };
        let c = self.m.choice.as_mut().expect("a deck choice");
        if c.decider() == viewer {
            c.cands[o as usize] = None;
            c.n = c.turn + 1;
        } else if let Some(k) = c.cands[o as usize].as_ref().map(|v| v.len()) {
            c.cands[o as usize] = Some(std::sync::Arc::from(vec![dealt; k]));
        }
    }

    /// A match whose first game has these seats (`MtgAz::new`'s, and the fuzz's own decks):
    /// `deck_priors` the hidden decklists' prior of each seat's deck (ENGINE.md 102; both or
    /// neither, with `gspec.hidden_decklists`). Every real deck must be inside its prior's
    /// support, or the real game would lie outside the other player's information.
    pub fn from_seats(
        db: &'static CardDb,
        gspec: GameSpec,
        format: Format,
        seats: &[Seat; 2],
        seed: u64,
        deck_priors: [Option<std::sync::Arc<dyn hidden_decks::DeckPrior>>; 2],
    ) -> Result<MtgAz, String> {
        if deck_priors.iter().any(|p| p.is_some() != gspec.hidden_decklists) {
            return Err("mtg: hidden decklists need a deck prior for each seat, and only they take one (ENGINE.md 102)".into());
        }
        for (p, prior) in deck_priors.iter().enumerate() {
            if let Some(pr) = prior {
                if let Some(&d) = seats[p].deck.iter().find(|&&d| pr.support().binary_search(&db.def(d).card_def(d)).is_err()) {
                    return Err(format!("mtg: p{p}'s {} is outside the support of its deck prior {} (ENGINE.md 102)", db.def(d).name, pr.name()));
                }
            }
        }
        let [a, b] = seats;
        let g = Game::with_sideboards(db, gspec, [&a.deck, &b.deck], [&a.sideboard, &b.sideboard], seed)?;
        let mut m = MatchState::new(db, format, gspec, seats, seed)?;
        m.deck_priors = deck_priors;
        let mut az = MtgAz { g, m, lab: None };
        az.set_known_pools();
        Ok(az)
    }
}

/// The `heuristic` bot as a zero-core `Bot`.
struct HeuristicBot(bots::Heuristic);

impl Bot<MtgAz> for HeuristicBot {
    fn choose(&mut self, g: &MtgAz, legal: &[u16]) -> u16 {
        // (the deck choice, ENGINE.md 107.2: candidate 0, the builder's best guess)
        if g.m.choice.is_some() {
            return legal[0];
        }
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

    /// A sealed spec is refused: it is game `mtg_eoe`'s (ENGINE.md 105).
    fn parse_spec(json: &str) -> Result<MtgSpec, String> {
        let s = MtgSpec::parse(json)?;
        s.check_game(Self::NAME)?;
        Ok(s)
    }

    fn new_game(spec: &MtgSpec, players: usize, seed: u64) -> Result<Self, String> {
        if players != 2 {
            return Err(format!("mtg: {players} players (two only)"));
        }
        // (A drafted match's spec carries the event's match seed; a lab game's start, its spot.)
        if spec.start.is_some() {
            return MtgAz::from_start(spec, seed);
        }
        MtgAz::new(spec, spec.seed.unwrap_or(seed))
    }

    fn num_players(&self) -> usize {
        2
    }

    fn to_move(&self) -> Option<usize> {
        // (one episode per game, ENGINE.md 121: the episode ended with the game)
        if self.m.between {
            return None;
        }
        if let Some(c) = &self.m.choice {
            return Some(c.decider() as usize);
        }
        match &self.m.sb {
            Some(sb) => Some(sb.decider() as usize),
            None => self.g.to_move().map(|p| p as usize),
        }
    }

    /// An aborted game (ENGINE.md 100.7: a number past the exact range, or
    /// past the object limit) has no result: zero-core's driver drops it (no
    /// samples) and its search values it by the network (the opt-in
    /// `SearchGame::aborted`). A best-of-three match ends with it.
    fn aborted(&self) -> bool {
        self.m.sb.is_none() && self.g.aborted()
    }

    fn legal_actions(&self, out: &mut Vec<u16>) {
        if let Some(c) = &self.m.choice {
            out.clear();
            out.extend(c.decision.options.iter().map(|o| o.action));
            return;
        }
        match &self.m.sb {
            Some(sb) => {
                out.clear();
                out.extend(sb.decision.options.iter().map(|o| o.action));
            }
            None => {
                self.g.legal_actions(out);
                if let Some(l) = &self.lab {
                    l.filter(&self.g, out);
                }
            }
        }
    }

    fn step(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        if self.lab.is_none() {
            return self.match_step(a, chance);
        }
        // a position-lab game (ENGINE.md 76): its line and rule follow the step (an ambiguous label is
        // an error, never a guess)
        if let Some(l) = self.lab.as_mut() {
            if let Some(e) = l.ambiguity(&self.g) {
                return Err(e);
            }
            l.before_step(&self.g, a);
        }
        self.match_step(a, chance)?;
        if let Some(l) = self.lab.as_mut() {
            l.after_step(&self.g);
        }
        Ok(())
    }

    fn force_chance(&mut self, forced: &[u64]) {
        self.g.force_chance(forced);
    }

    /// Best-of-one, and best-of-three with one episode per game (ENGINE.md
    /// 121): the game's; best-of-three otherwise: the match's (36.1).
    fn result(&self) -> [f32; MAX_SEATS] {
        let mut r = [0.0; MAX_SEATS];
        let v = match self.m.format {
            Format::Bo1 if self.g.is_over() => self.g.result_values(),
            Format::Bo3 if self.m.game_episodes => {
                if self.g.is_over() && self.m.sb.is_none() {
                    self.g.result_values()
                } else {
                    [0.5, 0.5]
                }
            }
            Format::Bo3 if self.m.over => self.m.result(),
            _ => [0.5, 0.5],
        };
        r[0] = v[0];
        r[1] = v[1];
        r
    }

    /// Best-of-one, and best-of-three with one episode per game (ENGINE.md
    /// 121): the game's life difference / 20. Best-of-three (ENGINE.md
    /// 36.6): 0.75 x the game-win difference / 2 + 0.25 x the current (at
    /// the end: the last) game's life margin.
    fn margin(&self, seat: usize) -> f32 {
        let me = self.g.life(seat as u8).max(0) as f32;
        let them = self.g.life(1 - seat as u8).max(0) as f32;
        let life = ((me - them) / 20.0).clamp(-1.0, 1.0);
        match self.m.format {
            Format::Bo1 => life,
            Format::Bo3 if self.m.game_episodes => life,
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
        // (the deck choice before the game, ENGINE.md 107.2: the other seat's deck dealt, nothing else)
        if self.m.choice.is_some() {
            self.determinize_deck_choice(viewer as u8, &mut r);
            self.m.rng = Rng::new(r.next_u64());
            return;
        }
        if self.m.format == Format::Bo3 && !self.m.over {
            self.sample_hidden_pool(viewer as u8, &mut r);
        }
        // Hidden decklists (ENGINE.md 102): the opponent's whole unseen deck from its prior.
        if self.hidden_decklists() {
            self.sample_hidden_deck(viewer as u8, &mut r, None, None);
        }
        if self.m.sb.is_none() {
            self.g.determinize(viewer as u8, &mut r);
        }
        self.m.rng = Rng::new(r.next_u64());
    }

    fn encode(&self, viewer: usize, out: &mut [f32]) {
        if self.m.choice.is_some() {
            encode::encode_deck_choice(self, viewer as u8, out);
        } else if self.m.sb.is_some() {
            encode::encode_sideboarding(self, viewer as u8, out);
        } else {
            self.g.encode_features(viewer as u8, out);
            encode::encode_match(self, viewer as u8, out);
        }
    }

    /// The turns of the match's games.
    fn rounds(&self) -> u32 {
        self.m.turns_before + if self.m.format == Format::Bo3 && (self.m.over || self.m.sb.is_some() || self.m.between) { 0 } else { self.g.turn().number }
    }

    /// Minor: which mana source to tap next while paying (ENGINE.md 22; zero-core searches it
    /// with the seat's `sims_minor` and never records it).
    fn minor_decision(&self) -> bool {
        self.m.choice.is_none()
            && self
                .g
            .pending_decision()
            .is_some_and(|d| matches!(d.kind, crate::decision::DecisionKind::PayMana | crate::decision::DecisionKind::PayPool))
    }

    /// The spectator view (`view.rs`): what the replay recorder stores at every step.
    fn export_state(&self) -> String {
        self.spectator_json()
    }

    /// The active player (zero-core's lookahead horizon 'next_turn', docs/ENGINE.md 80; the
    /// exported state's `active`); None while sideboarding (between the games of a match).
    fn active_seat(&self) -> Option<usize> {
        match self.m.sb {
            Some(_) => None,
            None => Some(self.g.turn().active as usize),
        }
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

    /// Two auxiliary targets (ENGINE.md 66, 102.4, zero-core docs/az.md 15), each counts per card
    /// identity (the `identity::D` identities of the encoding, ENGINE.md 22.2: the cube's, the
    /// legacy cards', the tokens'): `belief`, the decider's opponent's hand (the encoding frame's
    /// opponent's, ENGINE.md 100.5); `deck`, that player's unseen cards - their hand, library and
    /// face-down cards the decider does not know exactly (what `determinize` deals, and with hidden
    /// decklists the deck reader's target: a multinomial whose total is public). TRUE states -
    /// training labels that zero-core records with a sample when a run's `aux_belief_w` or
    /// `aux_deck_w` is on, never inputs: `encode` shows the viewer only what it knows (ENGINE.md
    /// 10). Empty (no target) while sideboarding, between the games of a match.
    const AUX: &'static [(&'static str, usize)] = &[("belief", identity::D), ("deck", identity::D)];

    fn aux_target(&self, viewer: usize, out: &mut [f32]) {
        if self.m.sb.is_some() || self.m.choice.is_some() {
            return;
        }
        let ids = identity::identities(self.g.db());
        // (The frame's opponent's hand: the decider's own while it decides
        // for the player it controls, ENGINE.md 100.5.)
        let seat = self.g.encoding_seat(viewer as u8);
        for &c in &self.g.player(1 - seat).hand {
            out[ids[self.g.obj(c).def as usize] as usize] += 1.0;
        }
        // `deck` (ENGINE.md 102.4): the frame opponent's cards the decider does not know exactly,
        // each by the card it is (a face-down permanent's card: `Game::ident`).
        let deck = &mut out[identity::D..2 * identity::D];
        for c in self.g.unknown_to(viewer as u8, 1 - seat) {
            deck[ids[self.g.ident(c) as usize] as usize] += 1.0;
        }
    }

    /// `determinize` dealt by the root's belief (ENGINE.md 69, 106; zero-core docs/search.md 3.8): the
    /// same constraints, the best-of-three hidden pool first; with hidden decklists and the `deck`
    /// target's tilt (`belief_tilts`, ENGINE.md 106) the opponent's deck drawn among the K cached decks
    /// by the deck prediction; then, with the hand's tilt, the opponent's hand's unknown slots drawn by
    /// the weights of `belief::deal_weights` (the head's distribution over their hand's cards against
    /// the dealt world's uniform deal, tilted and clipped). Between the games of a match (no hand to
    /// deal): `determinize`.
    fn determinize_with(&mut self, viewer: usize, rng: &mut zero_search::rng::Rng, belief: &zero_search::game::Belief) {
        self.determinize_belief(viewer, rng, belief);
    }

    /// The sealed deck choice before the game (ENGINE.md 107.2) is a value choice (zero-core
    /// docs/search.md 3.17): a seat with `value_choice_worlds` values each candidate by the network's value,
    /// from its own view, of the game started with it.
    fn value_choice(&self) -> bool {
        self.m.choice.is_some()
    }

    /// One zero-core episode per game of a best-of-three match (ENGINE.md 121: the spec's `"game_episodes"`).
    fn chained(&self) -> bool {
        self.m.game_episodes
    }

    /// The match after the episode's game (ENGINE.md 121, `matches.rs`): its result once over, else the
    /// sideboarding for the next game begun; a concession (zero-core's resign rule) first ends the game in
    /// progress (CR 104.3a).
    fn next_episode(&self, conceded: Option<usize>) -> Result<Next<Self>, String> {
        self.next_match_episode(conceded.map(|p| p as u8))
    }

    /// With one episode per game (ENGINE.md 121): only in a game (not while sideboarding or choosing a deck:
    /// no game is in progress to concede). Every other spec: every decision (as before).
    fn can_concede(&self) -> bool {
        !self.m.game_episodes || (self.m.sb.is_none() && self.m.choice.is_none() && !self.g.is_over())
    }
}
