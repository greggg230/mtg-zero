//! The zero-core game `team_draft` (DESIGN.md 2.3, ENGINE.md 41): a team cube
//! draft of the PowerLSV cube and the players' deckbuilding, up to the
//! matches, which are played elsewhere (the event's deferred terminal).
//!
//! * **Seats** `0..2T` (team size `T` = 1..4: zero-core's players = `2T`),
//!   seated A1 B1 A2 B2 ...: seat `s` is in team `s % 2`, player `s / 2` of it.
//! * **Packs**: the game seed shuffles the 541 draftable cards (the cube's IR
//!   card list less the basic lands); pack `(round r, opener s)` is the next
//!   `pack_size` cards; the rest is never drafted. Round 0 passes left (seat
//!   `s` to `s + 1`), round 1 right, round 2 left: every pass goes to an
//!   opponent.
//! * **Picks** are simultaneous within a pick round, asked in seat order,
//!   each hidden from the others. A pack is stored as its **removal order**:
//!   position `t` holds the card taken at pick `t`, the positions from
//!   `taken` on the cards still in it (kept sorted by identity).
//! * **Deckbuilding** (after the draft, seat by seat): one decision per pool
//!   card in the seat's build order (its identity = in the deck, `OUT` = in
//!   the sideboard; once the deck holds `DECK_SIZE` cards only `OUT`), then
//!   the five basic land counts (`COUNT + n`; the last is forced); every deck
//!   of exactly `DECK_SIZE` = 40 cards with at most `MAX_BASIC` of a basic
//!   land is reachable in exactly one way (ENGINE.md 42).
//! * **The deferred terminal**: after the last seat's last count `to_move()`
//!   is `None` and the event waits for its matches (`matches`,
//!   `deferred_payload`); `resolve` sets their results. `result`, `margin`
//!   panic until then (zero-core's deferred hook is M6 part 2).
//! * **Information** (DESIGN.md 2.3.2): a seat knows the packs it has seen
//!   (each view's contents, its own picks); after the draft it knows its
//!   team's union. `determinize` keeps exactly that and redraws the rest.
//! * **Keys**: `KEY_SEE(seat, identity)` for every card of the pack a seat
//!   receives or opens; `KEY_TEAM(team, pack << 24 | pick << 16 | identity)`
//!   at the end of the draft for every cell the team knows. After
//!   `determinize(viewer)` the keys private to other seats (teams) are one
//!   `HIDDEN` key per seat (team): the viewer's tree never branches on them.

use std::sync::{Arc, OnceLock};

use serde_json::{json, Value};
use zero_search::game::{Bot, SearchGame, MAX_SEATS};

use crate::db::{types, CardDb};
use crate::hidden::{key, key_kind, key_payload, key_player, HIDDEN};
use crate::ids::DefId;
use crate::mana::COLORS;
use crate::rng::{split64, Rng};

use super::decks::{self, DeckGen};
use super::identity;
use super::matches::{Format, MAX_COUNT};
use super::pools;

/// Action ids: `0..CARD_CAP` a card by identity (pick it; in deckbuilding, put
/// it in the deck), `OUT` (leave the card being decided in the sideboard),
/// `COUNT + n` (that many basic lands of the type being decided).
pub const CARD_CAP: u16 = identity::CUBE_CAP as u16;
pub const OUT: u16 = 768;
pub const COUNT: u16 = 800;
pub const COUNT_N: u16 = MAX_COUNT as u16 + 1;
pub const NUM_ACTIONS: usize = 1024;

/// The largest pack, number of packs and pool (the layout's slot counts).
pub const MAX_PACK: usize = 15;
pub const MAX_PACKS: usize = 3;
pub const MAX_POOL: usize = MAX_PACK * MAX_PACKS;
pub const MAX_TEAM: usize = 4;
pub const MAX_SEATS_DRAFT: usize = 2 * MAX_TEAM;

/// A deck has exactly 40 cards (ENGINE.md 42: a limited deck built from a
/// pool, a format restriction the user chose; CR 100.2b makes 40 a minimum
/// and allows any number of basic lands), at most 31 of a basic land (the
/// `SideboardCount` range of the match, ENGINE.md 36.2).
pub const DECK_SIZE: usize = decks::DECK_SIZE;
pub const MAX_BASIC: u8 = MAX_COUNT;

/// Key kinds (the `mtg` game's are 1..7).
pub const KEY_SEE: u64 = 16;
pub const KEY_TEAM: u64 = 17;

/// A pick that has not happened / a card that is no identity.
const NONE: u16 = u16::MAX;

// ------------------------------------------------------------------ the cube

/// The draftable cube: the cube's IR cards but the basic lands, by identity.
pub struct DraftCube {
    pub db: &'static CardDb,
    /// Identities of the draftable cards (sorted).
    pub cards: Vec<u16>,
    /// Identity -> definition (`DefId::MAX` for none).
    pub def: Vec<DefId>,
    /// The basic lands W U B R G: identities and definitions.
    pub basic_ids: [u16; 5],
    pub basic_defs: [DefId; 5],
    /// Identity -> the build-order key (land, colour group, mana value).
    pub build_key: Vec<(u8, u8, u8)>,
}

impl DraftCube {
    /// The definition of a card identity (a draftable card or a basic land).
    #[inline]
    pub fn def_of(&self, ident: u16) -> DefId {
        self.def[ident as usize]
    }

    /// The identity of a card definition.
    #[inline]
    pub fn ident_of(&self, def: DefId) -> u16 {
        identity::identities(self.db)[def as usize]
    }

    pub fn name(&self, ident: u16) -> &str {
        &self.db.def(self.def_of(ident)).name
    }
}

/// The PowerLSV draft cube (every non-basic cube card must be supported: an
/// unsupported one is an error, never skipped).
pub fn cube() -> Result<&'static DraftCube, String> {
    static CUBE: OnceLock<Result<DraftCube, String>> = OnceLock::new();
    CUBE.get_or_init(|| {
        let db = CardDb::cube();
        let ids = identity::identities(db);
        let names = identity::cube_names();
        let mut def = vec![DefId::MAX; identity::CUBE_CAP];
        let mut cards = Vec::new();
        let mut basic_ids = [0u16; 5];
        let mut basic_defs = [0 as DefId; 5];
        let mut bad = Vec::new();
        for (i, name) in names.iter().enumerate() {
            match db.expect_id(name) {
                Ok(d) => {
                    if ids[d as usize] as usize != i {
                        return Err(format!("team_draft: {name} has identity {} not {i}", ids[d as usize]));
                    }
                    def[i] = d;
                    if let Some(b) = decks::BASICS.iter().position(|&n| n == name) {
                        basic_ids[b] = i as u16;
                        basic_defs[b] = d;
                    } else {
                        cards.push(i as u16);
                    }
                }
                Err(e) => bad.push(e),
            }
        }
        if !bad.is_empty() {
            return Err(format!("team_draft: the cube has unsupported cards: {}", bad.join("; ")));
        }
        let build_key = (0..identity::CUBE_CAP)
            .map(|i| {
                if def[i] == DefId::MAX {
                    return (0, 0, 0);
                }
                let d = db.def(def[i]);
                if d.is(types::LAND) {
                    return (1, 0, 0);
                }
                let c = d.colors & COLORS;
                let group = match c.count_ones() {
                    0 => 6,
                    1 => c.trailing_zeros() as u8,
                    _ => 5,
                };
                (0, group, d.mana_value())
            })
            .collect();
        Ok(DraftCube { db, cards, def, basic_ids, basic_defs, build_key })
    })
    .as_ref()
    .map_err(|e| e.clone())
}

// ------------------------------------------------------------------ the spec

/// The spec of `team_draft` (JSON; every key optional):
/// `{"rounds": min(T, 3), "packs": 3, "pack_size": 15, "max_turns": 60, "format": "bo3"}`.
/// `rounds`: rounds of matches (rotations of the Latin square, at most `T`);
/// `max_turns` and `format`: the matches' (ENGINE.md 36).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftSpec {
    pub rounds: Option<u8>,
    pub packs: u8,
    pub pack_size: u8,
    pub max_turns: u32,
    pub format: Format,
}

impl Default for DraftSpec {
    fn default() -> Self {
        DraftSpec { rounds: None, packs: 3, pack_size: 15, max_turns: 60, format: Format::Bo3 }
    }
}

impl DraftSpec {
    pub fn parse(json: &str) -> Result<DraftSpec, String> {
        let mut s = DraftSpec::default();
        if json.trim().is_empty() {
            return Ok(s);
        }
        let v: Value = serde_json::from_str(json).map_err(|e| format!("team_draft spec: {e}"))?;
        let obj = v.as_object().ok_or("team_draft spec: not a JSON object")?;
        for (k, x) in obj {
            let int = || x.as_u64().ok_or_else(|| format!("team_draft spec: {k} must be a non-negative integer"));
            match k.as_str() {
                "rounds" => s.rounds = Some(int()?.min(255) as u8),
                "packs" => s.packs = int()?.min(255) as u8,
                "pack_size" => s.pack_size = int()?.min(255) as u8,
                "max_turns" => s.max_turns = int()? as u32,
                "format" => {
                    s.format = match x.as_str() {
                        Some("bo1") => Format::Bo1,
                        Some("bo3") => Format::Bo3,
                        _ => return Err(format!("team_draft spec: format must be \"bo1\" or \"bo3\", not {x}")),
                    }
                }
                other => return Err(format!("team_draft spec: unknown key {other:?}")),
            }
        }
        if !(1..=MAX_PACKS as u8).contains(&s.packs) {
            return Err(format!("team_draft spec: packs {} (1..={MAX_PACKS})", s.packs));
        }
        if !(2..=MAX_PACK as u8).contains(&s.pack_size) {
            return Err(format!("team_draft spec: pack_size {} (2..={MAX_PACK})", s.pack_size));
        }
        if s.rounds == Some(0) {
            return Err("team_draft spec: rounds must be at least 1".into());
        }
        if s.max_turns == 0 {
            return Err("team_draft spec: max_turns must be positive".into());
        }
        Ok(s)
    }
}

/// The settings of one event (the spec with the players).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Seats (`2T`) and team size.
    pub n: u8,
    pub t: u8,
    pub packs: u8,
    pub size: u8,
    pub rounds: u8,
    pub max_turns: u32,
    pub format: Format,
}

impl Event {
    pub fn new(spec: &DraftSpec, players: usize) -> Result<Event, String> {
        if players < 2 || players > MAX_SEATS_DRAFT || players % 2 != 0 {
            return Err(format!("team_draft: {players} players (2, 4, 6 or 8: two teams seated alternately)"));
        }
        let t = (players / 2) as u8;
        let rounds = spec.rounds.unwrap_or(t.min(3));
        if rounds > t {
            return Err(format!("team_draft: {rounds} rounds of matches for teams of {t} (at most {t})"));
        }
        let cube = cube()?;
        let need = players * spec.packs as usize * spec.pack_size as usize;
        if need > cube.cards.len() {
            return Err(format!("team_draft: {need} cards needed, the cube has {}", cube.cards.len()));
        }
        Ok(Event { n: players as u8, t, packs: spec.packs, size: spec.pack_size, rounds, max_turns: spec.max_turns, format: spec.format })
    }

    /// Packs of the event (`packs x n`); pack `p` = round `p / n`, opener `p % n`.
    #[inline]
    pub fn n_packs(&self) -> usize {
        self.packs as usize * self.n as usize
    }

    /// Cards of a pool.
    #[inline]
    pub fn pool_size(&self) -> usize {
        self.packs as usize * self.size as usize
    }

    /// Round `r` passes left (to the next seat) or right.
    #[inline]
    pub fn left(&self, r: usize) -> bool {
        r % 2 == 0
    }

    /// The seat holding pack `p` at pick `t`.
    #[inline]
    pub fn seat_at(&self, p: usize, t: usize) -> u8 {
        let n = self.n as usize;
        let (r, opener) = (p / n, p % n);
        (if self.left(r) { (opener + t) % n } else { (opener + n * MAX_PACK - t) % n }) as u8
    }

    /// The pack seat `s` holds at pick `t` of round `r`.
    #[inline]
    pub fn pack_at(&self, s: u8, r: usize, t: usize) -> usize {
        let n = self.n as usize;
        let s = s as usize;
        let opener = if self.left(r) { (s + n * MAX_PACK - t) % n } else { (s + t) % n };
        r * n + opener
    }

    /// The seats of team `team` as a bit mask.
    #[inline]
    pub fn team_mask(&self, team: u8) -> u16 {
        (0..self.n).filter(|s| s % 2 == team).fold(0, |m, s| m | 1 << s)
    }
}

// ------------------------------------------------------------------ the state

/// The draft and deckbuilding phases, and the deferred terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Draft,
    Build,
    Done,
}

/// One seat's deckbuilding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    /// The pool in build order (identities): spells by colour group (W, U, B,
    /// R, G, multicolour, colourless), then mana value, then identity; then
    /// lands by identity.
    pub order: Vec<u16>,
    /// The next card to decide (`order.len()`: the counts).
    pub k: u8,
    /// Bit i: `order[i]` is in the deck.
    pub inn: u64,
    /// Basic lands W U B R G.
    pub basics: [u8; 5],
    /// The basic type being counted (5: done).
    pub basic_k: u8,
}

impl Build {
    pub fn new(order: Vec<u16>) -> Build {
        Build { order, k: 0, inn: 0, basics: [0; 5], basic_k: 0 }
    }

    pub fn done(&self) -> bool {
        self.basic_k as usize >= 5
    }

    /// Cards in the deck so far.
    pub fn size(&self) -> usize {
        self.inn.count_ones() as usize + self.basics.iter().map(|&b| b as usize).sum::<usize>()
    }

    /// May the card being decided go in the deck (it holds fewer than
    /// `DECK_SIZE` pool cards; else the rest of the pool is forced `OUT`)?
    pub fn room(&self) -> bool {
        (self.inn.count_ones() as usize) < DECK_SIZE
    }

    /// The counts the type being decided may take: a deck of exactly
    /// `DECK_SIZE` stays reachable with at most `MAX_BASIC` of each type
    /// (never empty; the Forest count is forced).
    pub fn count_range(&self) -> (u8, u8) {
        let j = self.basic_k as usize;
        let before = self.inn.count_ones() as i64 + self.basics[..j].iter().map(|&b| b as i64).sum::<i64>();
        let after = (4 - j) as i64 * MAX_BASIC as i64;
        let lo = (DECK_SIZE as i64 - before - after).max(0);
        let hi = (MAX_BASIC as i64).min(DECK_SIZE as i64 - before);
        debug_assert!(lo <= hi, "a deck stays reachable");
        (lo as u8, hi.max(lo) as u8)
    }

    /// The deck's cards (identities; basics repeated), sorted.
    pub fn deck(&self, cube: &DraftCube) -> Vec<u16> {
        let mut v: Vec<u16> = (0..self.order.len()).filter(|&i| self.inn & (1 << i) != 0).map(|i| self.order[i]).collect();
        for (b, &n) in self.basics.iter().enumerate() {
            v.extend(std::iter::repeat_n(cube.basic_ids[b], n as usize));
        }
        v.sort_unstable();
        v
    }
}

/// The result of an event's matches.
#[derive(Clone, Debug, PartialEq)]
pub struct EventOutcome {
    /// Per match (in `matches` order): the result of its two players (A, B).
    pub results: Vec<[f32; 2]>,
    /// Match wins of team A and team B.
    pub wins: [u32; 2],
    /// Per team: 1 / 0 / 0.5.
    pub team: [f32; 2],
}

/// A match of the event (DESIGN.md 2.3.6): the A player is `mtg` seat 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchSetup {
    /// The round of matches (0..rounds).
    pub round: u8,
    /// The two seats (A, B).
    pub seats: [u8; 2],
    pub seed: u64,
    pub players: [MatchPlayer; 2],
}

/// One player of a match: its deck, sideboard (the rest of its pool), first
/// picks, and the cards its team saw elsewhere in the event (not in the
/// opponent's known pool); definitions, sorted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchPlayer {
    pub seat: u8,
    pub deck: Vec<DefId>,
    pub sideboard: Vec<DefId>,
    pub first_picks: Vec<DefId>,
    pub elsewhere: Vec<DefId>,
}

/// A team draft event (the `team_draft` game's state).
#[derive(Clone)]
pub struct TeamDraftAz {
    pub ev: Arc<Event>,
    /// Removal order of every pack (`pack * size + position`).
    pub(crate) order: Vec<u16>,
    /// Picks taken from each pack.
    pub(crate) taken: Vec<u8>,
    /// The cards never drafted (sorted).
    pub(crate) rest: Vec<u16>,
    /// The current pick round: round, pick, the seat to pick.
    pub(crate) round: u8,
    pub(crate) pick: u8,
    pub(crate) turn: u8,
    pub(crate) phase: Phase,
    pub(crate) builds: Vec<Build>,
    pub(crate) builder: u8,
    /// The match rotations (one per round of matches) and the matches' seed.
    pub(crate) rotations: Vec<u8>,
    pub(crate) match_seed: u64,
    /// The viewer `determinize` was called for (keys private to other seats
    /// are masked).
    pub(crate) observer: Option<u8>,
    /// Queued chance keys (`force_chance`) and how many were consumed.
    pub(crate) forced: Vec<u64>,
    pub(crate) fpos: usize,
    pub(crate) outcome: Option<Arc<EventOutcome>>,
}

impl TeamDraftAz {
    pub fn new(spec: &DraftSpec, players: usize, seed: u64) -> Result<TeamDraftAz, String> {
        let ev = Event::new(spec, players)?;
        let cube = cube()?;
        let mut rng = Rng::new(split64(seed ^ 0x7EA4_D2AF_0000_0001));
        let mut cards = cube.cards.clone();
        rng.shuffle(&mut cards);
        let s = ev.size as usize;
        let np = ev.n_packs();
        let mut order = cards[..np * s].to_vec();
        for p in 0..np {
            order[p * s..(p + 1) * s].sort_unstable();
        }
        let mut rest = cards[np * s..].to_vec();
        rest.sort_unstable();
        let mut g = TeamDraftAz {
            ev: Arc::new(ev),
            order,
            taken: vec![0; np],
            rest,
            round: 0,
            pick: 0,
            turn: 0,
            phase: Phase::Draft,
            builds: Vec::new(),
            builder: 0,
            rotations: Vec::new(),
            match_seed: 0,
            observer: None,
            forced: Vec::new(),
            fpos: 0,
            outcome: None,
        };
        let mut r2 = Rng::new(split64(seed ^ 0x7EA4_D2AF_0000_0002));
        g.draw_future(&mut r2);
        Ok(g)
    }

    /// The pairings and the matches' seed (future chance: redrawn by
    /// `determinize`).
    fn draw_future(&mut self, rng: &mut Rng) {
        let mut rot: Vec<u8> = (0..self.ev.t).collect();
        rng.shuffle(&mut rot);
        rot.truncate(self.ev.rounds as usize);
        self.rotations = rot;
        self.match_seed = rng.next_u64();
    }

    // -------------------------------------------------------------- reading

    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The current pick round (round, pick, the seat to pick).
    pub fn pick_round(&self) -> (u8, u8, u8) {
        (self.round, self.pick, self.turn)
    }

    pub fn builder(&self) -> u8 {
        self.builder
    }

    pub fn build(&self, seat: u8) -> Option<&Build> {
        self.builds.get(seat as usize)
    }

    pub fn observer(&self) -> Option<u8> {
        self.observer
    }

    pub fn rest(&self) -> &[u16] {
        &self.rest
    }

    pub fn rotations(&self) -> &[u8] {
        &self.rotations
    }

    /// Picks taken from pack `p`.
    #[inline]
    pub fn taken(&self, p: usize) -> usize {
        self.taken[p] as usize
    }

    /// The card at position `t` of pack `p` (its removal order).
    #[inline]
    pub fn cell(&self, p: usize, t: usize) -> u16 {
        self.order[p * self.ev.size as usize + t]
    }

    /// The cards still in pack `p` (sorted).
    pub fn pack(&self, p: usize) -> &[u16] {
        let s = self.ev.size as usize;
        &self.order[p * s + self.taken[p] as usize..(p + 1) * s]
    }

    /// The pack seat `s` holds in the current pick round (`None` after the
    /// draft).
    pub fn pack_held(&self, s: u8) -> Option<usize> {
        (self.phase == Phase::Draft).then(|| self.ev.pack_at(s, self.round as usize, self.pick as usize))
    }

    /// Has the pack `p` reached the seat holding it at pick `t` (a view)?
    #[inline]
    pub fn happened(&self, p: usize, t: usize) -> bool {
        if self.phase != Phase::Draft {
            return true;
        }
        let r = p / self.ev.n as usize;
        r < self.round as usize || (r == self.round as usize && t <= self.pick as usize)
    }

    /// Seat `s`'s picks in pick order: (identity, round, pick).
    pub fn picks(&self, s: u8) -> Vec<(u16, u8, u8)> {
        let ev = &self.ev;
        let mut v = Vec::with_capacity(ev.pool_size());
        for r in 0..ev.packs as usize {
            for t in 0..ev.size as usize {
                let p = ev.pack_at(s, r, t);
                if t < self.taken[p] as usize {
                    v.push((self.cell(p, t), r as u8, t as u8));
                }
            }
        }
        v
    }

    /// Seat `s`'s pool (identities, sorted).
    pub fn pool(&self, s: u8) -> Vec<u16> {
        let mut v: Vec<u16> = self.picks(s).into_iter().map(|x| x.0).collect();
        v.sort_unstable();
        v
    }

    /// Seat `s`'s first picks: the cards it took first from the packs it
    /// opened (sorted).
    pub fn first_picks(&self, s: u8) -> Vec<u16> {
        let n = self.ev.n as usize;
        let mut v: Vec<u16> = (0..self.ev.packs as usize)
            .map(|r| r * n + s as usize)
            .filter(|&p| self.taken[p] > 0)
            .map(|p| self.cell(p, 0))
            .collect();
        v.sort_unstable();
        v
    }

    /// The seats whose knowledge a viewer has: itself during the draft, its
    /// team after it (DESIGN.md 2.3.2).
    pub fn knowing(&self, viewer: u8) -> u16 {
        if self.phase == Phase::Draft {
            1 << viewer
        } else {
            self.ev.team_mask(viewer % 2)
        }
    }

    /// The picks at which seats of `mask` saw pack `p` (sorted).
    pub fn views(&self, p: usize, mask: u16) -> ([u8; MAX_PACK], usize) {
        let mut v = [0u8; MAX_PACK];
        let mut n = 0;
        for t in 0..self.ev.size as usize {
            if self.happened(p, t) && mask & (1 << self.ev.seat_at(p, t)) != 0 {
                v[n] = t as u8;
                n += 1;
            }
        }
        (v, n)
    }

    /// What seats `mask` know (DESIGN.md 2.3.2), canonically: per pack and
    /// view, the pack's contents at the view (sorted) and the pick made then
    /// (if made). Two states with equal knowledge for a viewer are the same
    /// to it.
    pub fn knowledge(&self, mask: u16) -> Vec<(u16, u8, Vec<u16>, u16)> {
        let s = self.ev.size as usize;
        let mut out = Vec::new();
        for p in 0..self.ev.n_packs() {
            let (v, n) = self.views(p, mask);
            for &t in &v[..n] {
                let t = t as usize;
                let mut set: Vec<u16> = (t..s).map(|i| self.cell(p, i)).collect();
                set.sort_unstable();
                let own = if t < self.taken[p] as usize { self.cell(p, t) } else { NONE };
                out.push((p as u16, t as u8, set, own));
            }
        }
        out
    }

    /// The cells (pack, pick) whose card the seats `mask` know exactly: the
    /// picks between two of their views and after their last view are known
    /// only as a set; the cells before their first view of a pack (and every
    /// cell of a pack they have not seen) not at all. After the draft, for a
    /// team: every cell but the opposing openers' first picks.
    pub fn known_cells(&self, mask: u16) -> Vec<(u8, u8)> {
        let s = self.ev.size as usize;
        let mut out = Vec::new();
        for p in 0..self.ev.n_packs() {
            let (v, n) = self.views(p, mask);
            let taken = self.taken[p] as usize;
            for i in 0..n {
                let t = v[i] as usize;
                // The pick made at a view.
                if t < taken {
                    out.push((p as u8, t as u8));
                } else {
                    continue;
                }
                // The one cell between this view and the next (or the pack's
                // last cell after the last view): the card that disappeared.
                let next = if i + 1 < n { v[i + 1] as usize } else { s };
                if next == t + 2 {
                    out.push((p as u8, (t + 1) as u8));
                }
            }
        }
        out
    }

    /// The seat to decide (`None` at the deferred terminal).
    pub fn decider(&self) -> Option<u8> {
        match self.phase {
            Phase::Draft => Some(self.turn),
            Phase::Build => Some(self.builder),
            Phase::Done => None,
        }
    }

    /// The legal actions of the pending decision (sorted).
    pub fn legal(&self, out: &mut Vec<u16>) {
        out.clear();
        match self.phase {
            Phase::Draft => {
                let p = self.ev.pack_at(self.turn, self.round as usize, self.pick as usize);
                out.extend_from_slice(self.pack(p));
            }
            Phase::Build => {
                let b = &self.builds[self.builder as usize];
                if (b.k as usize) < b.order.len() {
                    // (A full deck forces the rest of the pool out: a single
                    // option, as a pack's last card.)
                    if b.room() {
                        out.push(b.order[b.k as usize]);
                    }
                    out.push(OUT);
                } else {
                    let (lo, hi) = b.count_range();
                    out.extend((lo..=hi).map(|n| COUNT + n as u16));
                }
            }
            Phase::Done => {}
        }
    }

    // -------------------------------------------------------------- keys

    /// Is a key private to seat `s` complete for the observer?
    #[inline]
    fn sees(&self, s: u8) -> bool {
        self.observer.is_none_or(|o| o == s)
    }

    fn emit(&mut self, k: u64, chance: &mut Vec<u64>) {
        chance.push(k);
        if self.fpos < self.forced.len() {
            self.fpos += 1;
        }
    }

    /// The packs of the current pick round reach their seats: each seat sees
    /// its pack (`KEY_SEE`). At pick 0 (the packs opened), forced keys are
    /// applied first (`force_chance`).
    fn arrivals(&mut self, chance: &mut Vec<u64>) {
        if self.pick == 0 && self.fpos < self.forced.len() {
            self.force_openings();
        }
        for s in 0..self.ev.n {
            let p = self.ev.pack_at(s, self.round as usize, self.pick as usize);
            if self.sees(s) {
                let s0 = self.ev.size as usize;
                for i in p * s0 + self.taken[p] as usize..(p + 1) * s0 {
                    let c = self.order[i];
                    self.emit(key(KEY_SEE, s, c as u64), chance);
                }
            } else {
                self.emit(key(KEY_SEE, s, HIDDEN), chance);
            }
        }
    }

    /// Forced openings: the queued `KEY_SEE` keys of a seat whose pack opens
    /// now name cards to put in it, taken from places nobody has seen (the
    /// undrafted cards, a later round's pack, another pack opening now) or,
    /// for the observer, any place it has not seen; the card swapped out is
    /// the smallest one of the pack that is not forced. Impossible keys fall
    /// back (the world's own card).
    fn force_openings(&mut self) {
        let ev = self.ev.clone();
        let s0 = ev.size as usize;
        let n = ev.n as usize;
        let r = self.round as usize;
        // The places a forced card may come from: (flat index or rest), by card.
        let mut free = vec![false; self.order.len()];
        for p in 0..ev.n_packs() {
            if p / n >= r {
                for i in p * s0..(p + 1) * s0 {
                    free[i] = true;
                }
            }
        }
        if let Some(o) = self.observer {
            let mask = 1u16 << o;
            for p in 0..ev.n_packs() {
                if p / n >= r {
                    continue;
                }
                let (v, nv) = self.views(p, mask);
                let first = if nv == 0 { s0 } else { v[0] as usize };
                for i in p * s0..p * s0 + first {
                    free[i] = true;
                }
            }
        }
        let mut off = self.fpos;
        for s in 0..ev.n {
            let cnt = if self.sees(s) { s0 } else { 1 };
            let end = (off + cnt).min(self.forced.len());
            let want: Vec<u16> = self.forced[off.min(end)..end]
                .iter()
                .filter(|&&k| key_kind(k) == KEY_SEE && key_player(k) == s && key_payload(k) != HIDDEN)
                .map(|&k| key_payload(k) as u16)
                .collect();
            off += cnt;
            if want.is_empty() {
                continue;
            }
            let p = r * n + s as usize;
            for &c in &want {
                let base = p * s0;
                if self.order[base..base + s0].contains(&c) {
                    continue;
                }
                // Where is c?
                let from_rest = self.rest.binary_search(&c).ok();
                let from_cell = if from_rest.is_none() {
                    (0..self.order.len()).find(|&i| self.order[i] == c && free[i] && !(base..base + s0).contains(&i))
                } else {
                    None
                };
                if from_rest.is_none() && from_cell.is_none() {
                    continue;
                }
                let Some(j) = (base..base + s0).filter(|&i| !want.contains(&self.order[i])).min_by_key(|&i| self.order[i]) else {
                    continue;
                };
                let x = self.order[j];
                self.order[j] = c;
                match (from_rest, from_cell) {
                    (Some(i), _) => {
                        self.rest.remove(i);
                        let at = self.rest.binary_search(&x).unwrap_or_else(|e| e);
                        self.rest.insert(at, x);
                    }
                    (None, Some(i)) => self.order[i] = x,
                    _ => unreachable!(),
                }
            }
        }
        // Keep every unpicked part sorted.
        self.sort_packs();
    }

    fn sort_packs(&mut self) {
        let s0 = self.ev.size as usize;
        for p in 0..self.ev.n_packs() {
            let a = p * s0 + self.taken[p] as usize;
            self.order[a..(p + 1) * s0].sort_unstable();
        }
    }

    /// The draft ends: each team learns what its seats saw (`KEY_TEAM`), and
    /// deckbuilding begins.
    fn end_draft(&mut self, chance: &mut Vec<u64>) {
        self.phase = Phase::Build;
        for team in 0..2u8 {
            if self.observer.is_none_or(|o| o % 2 == team) {
                for (p, t) in self.known_cells(self.ev.team_mask(team)) {
                    let c = self.cell(p as usize, t as usize);
                    let payload = (p as u64) << 24 | (t as u64) << 16 | c as u64;
                    self.emit(key(KEY_TEAM, team, payload), chance);
                }
            } else {
                self.emit(key(KEY_TEAM, team, HIDDEN), chance);
            }
        }
        let cube = cube().expect("the cube was loaded");
        self.builds = (0..self.ev.n).map(|s| Build::new(build_order(cube, &self.pool(s)))).collect();
        self.builder = 0;
    }

    // -------------------------------------------------------------- steps

    /// Apply a legal action; push the chance keys it reveals.
    pub fn apply(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        match self.phase {
            Phase::Draft => self.apply_pick(a, chance),
            Phase::Build => self.apply_build(a),
            Phase::Done => Err("team_draft: the event is over (its matches are pending or played)".into()),
        }
    }

    fn apply_pick(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        let ev = self.ev.clone();
        let s0 = ev.size as usize;
        let p = ev.pack_at(self.turn, self.round as usize, self.pick as usize);
        let lo = p * s0 + self.taken[p] as usize;
        let hi = (p + 1) * s0;
        let Ok(i) = self.order[lo..hi].binary_search(&a) else {
            return Err(format!("team_draft: seat {} cannot pick {a}: not in its pack", self.turn));
        };
        // The pick goes to position `taken`; the rest stays sorted.
        self.order[lo..=lo + i].rotate_right(1);
        self.taken[p] += 1;
        self.turn += 1;
        if self.turn < ev.n {
            return Ok(());
        }
        self.turn = 0;
        self.pick += 1;
        if self.pick as usize == s0 {
            self.pick = 0;
            self.round += 1;
            if self.round == ev.packs {
                self.round = ev.packs - 1;
                self.pick = ev.size - 1;
                self.end_draft(chance);
                return Ok(());
            }
        }
        self.arrivals(chance);
        Ok(())
    }

    fn apply_build(&mut self, a: u16) -> Result<(), String> {
        let n = self.ev.n;
        let s = self.builder;
        let b = &mut self.builds[s as usize];
        if (b.k as usize) < b.order.len() {
            let c = b.order[b.k as usize];
            if a == c && b.room() {
                b.inn |= 1 << b.k;
            } else if a != OUT {
                return Err(format!("team_draft: seat {s} deciding {c}: action {a} (the card while the deck has room, or OUT)"));
            }
            b.k += 1;
            return Ok(());
        }
        let (lo, hi) = b.count_range();
        if !(COUNT + lo as u16..=COUNT + hi as u16).contains(&a) {
            return Err(format!("team_draft: seat {s} basic count: action {a} not in {lo}..={hi}"));
        }
        b.basics[b.basic_k as usize] = (a - COUNT) as u8;
        b.basic_k += 1;
        if b.done() {
            self.builder += 1;
            if self.builder == n {
                self.phase = Phase::Done;
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------- determinize

    /// Redraw what `viewer` does not know (DESIGN.md 2.3.2); set it as the
    /// observer.
    pub fn redraw(&mut self, viewer: u8, rng: &mut Rng) {
        let ev = self.ev.clone();
        let s0 = ev.size as usize;
        let mask = self.knowing(viewer);
        let mut free: Vec<usize> = Vec::with_capacity(self.order.len());
        let mut segs: Vec<(usize, usize)> = Vec::new();
        for p in 0..ev.n_packs() {
            let base = p * s0;
            let (v, n) = self.views(p, mask);
            if n == 0 {
                free.extend(base..base + s0);
                continue;
            }
            free.extend(base..base + v[0] as usize);
            for i in 0..n {
                let a = v[i] as usize + 1;
                let b = if i + 1 < n { v[i + 1] as usize } else { s0 };
                if b > a + 1 {
                    segs.push((base + a, base + b));
                }
            }
        }
        // The unseen cards over the unseen places (the undrafted cards
        // included): a partial Fisher-Yates.
        let mut cards: Vec<u16> = free.iter().map(|&i| self.order[i]).chain(self.rest.iter().copied()).collect();
        let len = cards.len();
        for (i, &cell) in free.iter().enumerate() {
            let j = i + rng.below((len - i) as u32) as usize;
            cards.swap(i, j);
            self.order[cell] = cards[i];
        }
        let mut rest = cards[free.len()..].to_vec();
        rest.sort_unstable();
        self.rest = rest;
        for (a, b) in segs {
            rng.shuffle(&mut self.order[a..b]);
        }
        self.sort_packs();
        if self.phase != Phase::Draft {
            let cube = cube().expect("the cube was loaded");
            for o in (0..ev.n).filter(|o| o % 2 != viewer % 2) {
                let order = build_order(cube, &self.pool(o));
                let mut b = Build::new(order);
                let finished = self.phase == Phase::Done || o < self.builder;
                if finished {
                    // The heuristic deck of the part of its pool the viewer
                    // knows (the same in every world: nothing of the real deck),
                    // its unseen first picks in the sideboard.
                    let fp = self.first_picks(o);
                    let known: Vec<u16> = b.order.iter().copied().filter(|c| fp.binary_search(c).is_err()).collect();
                    let deck = heuristic_deck(cube, &known);
                    set_build(cube, &mut b, &deck);
                }
                // (An opponent building now restarts: its decisions are
                // hidden; one not yet building has only its order.)
                self.builds[o as usize] = b;
            }
        }
        self.draw_future(rng);
        self.observer = Some(viewer);
    }

    // -------------------------------------------------------------- the matches

    /// Is the event at its deferred terminal, waiting for its matches?
    pub fn deferred(&self) -> bool {
        self.phase == Phase::Done && self.outcome.is_none()
    }

    pub fn outcome(&self) -> Option<&EventOutcome> {
        self.outcome.as_deref()
    }

    /// The matches' pairings: (round, A seat, B seat), round by round, A
    /// player by A player.
    pub fn pairings(&self) -> Vec<(u8, u8, u8)> {
        let t = self.ev.t;
        let mut v = Vec::new();
        for (k, &d) in self.rotations.iter().enumerate() {
            for i in 0..t {
                v.push((k as u8, 2 * i, 2 * ((i + d) % t) + 1));
            }
        }
        v
    }

    /// Seat `s`'s deck and sideboard (definitions, sorted) once built.
    pub fn deck_and_sideboard(&self, s: u8) -> Option<(Vec<DefId>, Vec<DefId>)> {
        let b = self.builds.get(s as usize).filter(|b| b.done())?;
        let cube = cube().ok()?;
        let mut deck: Vec<DefId> = b.deck(cube).iter().map(|&c| cube.def_of(c)).collect();
        deck.sort_unstable();
        let mut side: Vec<DefId> =
            (0..b.order.len()).filter(|&i| b.inn & (1 << i) == 0).map(|i| cube.def_of(b.order[i])).collect();
        side.sort_unstable();
        Some((deck, side))
    }

    /// The cards a team knows the place of after the draft (every drafted
    /// card but the opposing first picks), sorted identities.
    pub fn team_known(&self, team: u8) -> Vec<u16> {
        let mut v: Vec<u16> = self.known_cells(self.ev.team_mask(team)).into_iter().map(|(p, t)| self.cell(p as usize, t as usize)).collect();
        v.sort_unstable();
        v
    }

    /// The event's matches (DESIGN.md 2.3.6), at the deferred terminal.
    pub fn matches(&self) -> Result<Vec<MatchSetup>, String> {
        if self.phase != Phase::Done {
            return Err("team_draft: the matches exist once every deck is built".into());
        }
        let cube = cube()?;
        let defs = |v: &[u16]| -> Vec<DefId> {
            let mut d: Vec<DefId> = v.iter().map(|&c| cube.def_of(c)).collect();
            d.sort_unstable();
            d
        };
        let known = [self.team_known(0), self.team_known(1)];
        let player = |s: u8, o: u8| -> MatchPlayer {
            let (deck, sideboard) = self.deck_and_sideboard(s).expect("built");
            let fp = self.first_picks(o);
            let opp_known: Vec<u16> = self.pool(o).into_iter().filter(|c| fp.binary_search(c).is_err()).collect();
            let elsewhere: Vec<u16> = known[(s % 2) as usize].iter().copied().filter(|c| opp_known.binary_search(c).is_err()).collect();
            MatchPlayer { seat: s, deck, sideboard, first_picks: defs(&self.first_picks(s)), elsewhere: defs(&elsewhere) }
        };
        Ok(self
            .pairings()
            .into_iter()
            .enumerate()
            .map(|(m, (round, a, b))| MatchSetup {
                round,
                seats: [a, b],
                seed: split64(self.match_seed ^ (m as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                players: [player(a, b), player(b, a)],
            })
            .collect())
    }

    /// The matches as JSON (card names; DESIGN.md 2.3.6): what part 2's
    /// deferred hook hands to the match driver.
    pub fn deferred_payload(&self) -> Result<String, String> {
        let cube = cube()?;
        let names = |v: &[DefId]| -> Vec<&str> { v.iter().map(|&d| cube.db.def(d).name.as_str()).collect() };
        let fmt = match self.ev.format {
            Format::Bo1 => "bo1",
            Format::Bo3 => "bo3",
        };
        let ms: Vec<Value> = self
            .matches()?
            .iter()
            .map(|m| {
                let pl: Vec<Value> = m
                    .players
                    .iter()
                    .map(|p| {
                        json!({"seat": p.seat, "deck": names(&p.deck), "sideboard": names(&p.sideboard),
                               "first_picks": names(&p.first_picks), "elsewhere": names(&p.elsewhere)})
                    })
                    .collect();
                json!({"round": m.round, "seats": m.seats, "seed": m.seed, "players": pl})
            })
            .collect();
        Ok(json!({"event": {"seats": self.ev.n, "team_size": self.ev.t, "rounds": self.ev.rounds,
                            "max_turns": self.ev.max_turns, "format": fmt},
                  "matches": ms})
        .to_string())
    }

    /// The matches' results (per match, in `matches` order: its A and B
    /// player's 1 / 0 / 0.5): the event's outcome.
    pub fn resolve(&mut self, results: &[[f32; 2]]) -> Result<(), String> {
        if !self.deferred() {
            return Err("team_draft: no pending matches (not at the deferred terminal, or already resolved)".into());
        }
        let n = self.rotations.len() * self.ev.t as usize;
        if results.len() != n {
            return Err(format!("team_draft: {} results for {n} matches", results.len()));
        }
        let mut wins = [0u32; 2];
        for r in results {
            match (r[0], r[1]) {
                (a, b) if a == 1.0 && b == 0.0 => wins[0] += 1,
                (a, b) if a == 0.0 && b == 1.0 => wins[1] += 1,
                (a, b) if a == 0.5 && b == 0.5 => {}
                _ => return Err(format!("team_draft: a match result {r:?} (1/0, 0/1 or 0.5/0.5)")),
            }
        }
        let team = match wins[0].cmp(&wins[1]) {
            std::cmp::Ordering::Greater => [1.0, 0.0],
            std::cmp::Ordering::Less => [0.0, 1.0],
            std::cmp::Ordering::Equal => [0.5, 0.5],
        };
        self.outcome = Some(Arc::new(EventOutcome { results: results.to_vec(), wins, team }));
        Ok(())
    }

    /// `resolve` from JSON: `[[a, b], ...]` or `[{"result": [a, b], ...}, ...]`.
    pub fn resolve_json(&mut self, s: &str) -> Result<(), String> {
        let v: Value = serde_json::from_str(s).map_err(|e| format!("team_draft results: {e}"))?;
        let a = v.as_array().ok_or("team_draft results: a list")?;
        let mut out = Vec::with_capacity(a.len());
        for x in a {
            let r = x.get("result").unwrap_or(x);
            let r = r.as_array().filter(|r| r.len() == 2).ok_or("team_draft results: each a pair [a, b]")?;
            let f = |y: &Value| y.as_f64().map(|z| z as f32).ok_or("team_draft results: numbers");
            out.push([f(&r[0])?, f(&r[1])?]);
        }
        self.resolve(&out)
    }

    /// The outcome per seat (zero-core's convention: sums to 1, the team's
    /// 1 / 0 / 0.5 divided by the team size).
    fn seat_values(&self) -> [f32; MAX_SEATS] {
        let mut r = [0.0; MAX_SEATS];
        let n = self.ev.n as usize;
        match (&self.outcome, self.phase) {
            (Some(o), _) => {
                for (s, x) in r.iter_mut().enumerate().take(n) {
                    *x = o.team[s % 2] / self.ev.t as f32;
                }
            }
            (None, Phase::Done) => panic!(
                "team_draft: the event's matches have not been played (a deferred result: zero-core's deferred hook is \
                 M6 part 2; resolve the event first)"
            ),
            (None, _) => r[..n].fill(1.0 / n as f32),
        }
        r
    }

    /// Checks the state's invariants (tests, fuzzing).
    pub fn check_invariants(&self) -> Result<(), String> {
        let ev = &self.ev;
        let cube = cube()?;
        let s0 = ev.size as usize;
        let mut seen = vec![0u8; identity::CUBE_CAP];
        for &c in self.order.iter().chain(&self.rest) {
            if c as usize >= identity::CUBE_CAP || cube.def[c as usize] == DefId::MAX || cube.basic_ids.contains(&c) {
                return Err(format!("not a draftable card: {c}"));
            }
            seen[c as usize] += 1;
        }
        for &c in &cube.cards {
            if seen[c as usize] != 1 {
                return Err(format!("card {c} appears {} times", seen[c as usize]));
            }
        }
        if !self.rest.windows(2).all(|w| w[0] < w[1]) {
            return Err("the undrafted cards are not sorted".into());
        }
        for p in 0..ev.n_packs() {
            let r = p / ev.n as usize;
            let want = match self.phase {
                Phase::Draft if r < self.round as usize => s0,
                Phase::Draft if r > self.round as usize => 0,
                Phase::Draft => self.pick as usize + (ev.seat_at(p, self.pick as usize) < self.turn) as usize,
                _ => s0,
            };
            if self.taken[p] as usize != want {
                return Err(format!("pack {p}: {} picks taken, {want} expected", self.taken[p]));
            }
            if !self.pack(p).windows(2).all(|w| w[0] < w[1]) {
                return Err(format!("pack {p}: the cards left are not sorted"));
            }
        }
        if self.phase != Phase::Draft {
            if self.builds.len() != ev.n as usize {
                return Err("builds".into());
            }
            for s in 0..ev.n {
                let b = &self.builds[s as usize];
                if b.order != build_order(cube, &self.pool(s)) {
                    return Err(format!("seat {s}: build order is not its pool's"));
                }
                if b.order.len() != ev.pool_size() {
                    return Err(format!("seat {s}: a pool of {}", b.order.len()));
                }
                let finished = self.phase == Phase::Done || s < self.builder;
                if b.inn.count_ones() as usize > DECK_SIZE || b.size() > DECK_SIZE {
                    return Err(format!("seat {s}: {} pool cards, {} cards", b.inn.count_ones(), b.size()));
                }
                if finished && (!b.done() || b.size() != DECK_SIZE) {
                    return Err(format!("seat {s}: deck of {} cards (done {})", b.size(), b.done()));
                }
                if s > self.builder && self.phase == Phase::Build && (b.k > 0 || b.basic_k > 0) {
                    return Err(format!("seat {s} builds before its turn"));
                }
                if b.basics.iter().any(|&x| x > MAX_BASIC) {
                    return Err(format!("seat {s}: basics {:?}", b.basics));
                }
            }
        }
        Ok(())
    }
}

/// A pool in build order (DESIGN.md 2.3.3).
pub fn build_order(cube: &DraftCube, pool: &[u16]) -> Vec<u16> {
    let mut v = pool.to_vec();
    v.sort_by_key(|&c| (cube.build_key[c as usize], c));
    v
}

/// The heuristic deckbuilder's deck (`pools::build_deck`) as a finished
/// build: the deck redrawn for an opponent who has built (its deck is
/// hidden), and the `consensus` bot's deck.
pub fn heuristic_build(cube: &DraftCube, b: &mut Build) {
    let deck = heuristic_deck(cube, &b.order);
    set_build(cube, b, &deck);
}

/// `pools::build_deck`'s deck of a pool (identities, any order), memoised
/// per thread: `determinize` asks it for the same known pools in every
/// world of a search.
pub fn heuristic_deck(cube: &DraftCube, pool: &[u16]) -> std::sync::Arc<[DefId]> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    thread_local! {
        static MEMO: RefCell<HashMap<Vec<u16>, std::sync::Arc<[DefId]>>> = RefCell::new(HashMap::new());
    }
    let mut key = pool.to_vec();
    key.sort_unstable();
    if let Some(d) = MEMO.with(|m| m.borrow().get(&key).cloned()) {
        return d;
    }
    let defs: Vec<DefId> = key.iter().map(|&c| cube.def_of(c)).collect();
    let (deck, _) = pools::build_deck(cube.db, &defs, &DeckGen::default());
    let deck: std::sync::Arc<[DefId]> = deck.into();
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() >= 4096 {
            m.clear();
        }
        m.insert(key, deck.clone());
    });
    deck
}

/// A finished build holding `deck` (its pool cards in, its basic lands).
fn set_build(cube: &DraftCube, b: &mut Build, deck: &[DefId]) {
    b.inn = 0;
    for (i, &c) in b.order.iter().enumerate() {
        if deck.contains(&cube.def_of(c)) {
            b.inn |= 1 << i;
        }
    }
    for (j, &d) in cube.basic_defs.iter().enumerate() {
        b.basics[j] = deck.iter().filter(|&&x| x == d).count().min(MAX_BASIC as usize) as u8;
    }
    b.k = b.order.len() as u8;
    b.basic_k = 5;
}

// ------------------------------------------------------------------ zero-core

impl SearchGame for TeamDraftAz {
    type Spec = DraftSpec;
    const NAME: &'static str = "team_draft";
    const NUM_ACTIONS: usize = NUM_ACTIONS;
    const NUM_FEATURES: usize = super::draft_encode::NUM_FEATURES;
    const MIN_PLAYERS: usize = 2;
    const MAX_PLAYERS: usize = MAX_SEATS_DRAFT;

    fn parse_spec(json: &str) -> Result<DraftSpec, String> {
        DraftSpec::parse(json)
    }

    fn new_game(spec: &DraftSpec, players: usize, seed: u64) -> Result<Self, String> {
        TeamDraftAz::new(spec, players, seed)
    }

    fn num_players(&self) -> usize {
        self.ev.n as usize
    }

    fn to_move(&self) -> Option<usize> {
        self.decider().map(|s| s as usize)
    }

    fn legal_actions(&self, out: &mut Vec<u16>) {
        self.legal(out);
    }

    fn step(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        self.apply(a, chance)
    }

    fn force_chance(&mut self, forced: &[u64]) {
        self.forced.clear();
        self.forced.extend_from_slice(forced);
        self.fpos = 0;
    }

    /// The team's 1 / 0 / 0.5 divided by the team size; **panics** at the
    /// deferred terminal before `resolve` (DESIGN.md 2.3.4).
    fn result(&self) -> [f32; MAX_SEATS] {
        self.seat_values()
    }

    /// (Team match wins - opponent match wins) / matches; panics at the
    /// deferred terminal before `resolve`.
    fn margin(&self, seat: usize) -> f32 {
        match &self.outcome {
            Some(o) => {
                let m = o.results.len().max(1) as f32;
                (o.wins[seat % 2] as f32 - o.wins[1 - seat % 2] as f32) / m
            }
            None => {
                let _ = self.seat_values();
                0.0
            }
        }
    }

    /// The team's match wins (reports).
    fn score(&self, seat: usize) -> f32 {
        self.outcome.as_ref().map_or(0.0, |o| o.wins[seat % 2] as f32)
    }

    fn determinize(&mut self, viewer: usize, rng: &mut zero_search::rng::Rng) {
        let mut r = Rng::new(rng.next_u64());
        self.redraw(viewer as u8, &mut r);
    }

    fn encode(&self, viewer: usize, out: &mut [f32]) {
        super::draft_encode::encode(self, viewer as u8, out);
    }

    /// Pick rounds played.
    fn rounds(&self) -> u32 {
        match self.phase {
            Phase::Draft => self.round as u32 * self.ev.size as u32 + self.pick as u32,
            _ => self.ev.packs as u32 * self.ev.size as u32,
        }
    }

    fn bot_names() -> &'static [&'static str] {
        &["consensus"]
    }

    fn make_bot(_spec: &DraftSpec, name: &str, _seed: u64) -> Option<Box<dyn Bot<Self>>> {
        match name {
            "consensus" => Some(Box::new(super::draft_bots::Consensus::default())),
            _ => None,
        }
    }
}
