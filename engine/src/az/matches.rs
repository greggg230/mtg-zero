//! Matches (ENGINE.md 36): the `mtg` episode is a match. Best-of-one (the
//! spec's `"format": "bo1"`, the default: one game, exactly the game of
//! M5a-M8a) or best-of-three (`"bo3"`): games until a player has won two or
//! three games have been played (a drawn game counts as a game; a match can
//! be a draw), with sideboarding between games (CR 100.4; in limited the
//! sideboard is the rest of the player's pool, 100.4b, and a deck is at
//! least 40 cards, 100.2b). The terminal result is the match's: 1 / 0 to
//! the player with more game wins, 0.5 each on equal wins.
//!
//! * Who chooses who takes the first turn (CR 103.1): in game 1 a player
//!   chosen at random (or the spec's `chooser`); in a later game the loser
//!   of the previous game, or after a drawn game the player who made the
//!   choice in it.
//! * Sideboarding (`Sideboard`, `SideboardCount` decisions): the next
//!   game's chooser first, then the other player, one after the other (a
//!   simultaneous choice made sequential: the first player's committed deck
//!   is not shown to the second, zero-core's pattern; with this part's open
//!   decklists the hiding is nominal, ENGINE.md 36.10). Each player's list
//!   is their pool's cards by definition, then the five basic lands; the
//!   player changes entries in increasing list order (one decision per
//!   single card, two for a basic land or a card with several copies: the
//!   entry, then its new count), `PASS` when done: every legal deck (40 to
//!   `MAX_DECK` cards) is reachable in exactly one way.
//! * The next game starts after both: decks and sideboards from the
//!   choices, its seed from the match's RNG (which `determinize` reseeds).
//! * The cards each player revealed (made public: `KEY_REVEAL`) in earlier
//!   games are kept for the encoding.

use std::sync::Arc;

use crate::db::CardDb;
use crate::decision::{act, Choice, Decision, DecisionKind, Opt};
use crate::game::{Game, GameSpec};
use crate::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_REVEAL};
use crate::ids::{opp, DefId, PlayerId};
use crate::rng::Rng;

use super::identity;
use super::pools::basic_ids;

/// The least deck size of limited (CR 100.2b).
pub const MIN_DECK: usize = 40;
/// The largest deck sideboarding may build (no rule caps a deck; an engine
/// bound so the decisions stay finite and small; ENGINE.md 36.2).
pub const MAX_DECK: usize = 60;
/// The largest count a `SideboardCount` sets (`NUMBER` ids 0..31).
pub const MAX_COUNT: u8 = (act::NUMBER_N - 1) as u8;
/// Games and wins of a best-of-three.
pub const BO3_GAMES: u8 = 3;
pub const BO3_WINS: u8 = 2;

/// A match's format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// One game (M5a-M8a's episode).
    Bo1,
    /// Best of three, sideboarding between games.
    Bo3,
}

/// A player's pool in a match and the list sideboarding names
/// (`LIBRARY` + k = entry k).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchPool {
    /// (definition, copies in the pool): the pool's cards by definition
    /// (basic lands are not pool cards), then the five basic lands (W U B R
    /// G; copies `MAX_COUNT`: any number, CR 100.2b).
    pub list: Vec<(DefId, u8)>,
    /// Entries before the basics.
    pub n_cards: usize,
    /// The first picks (data only in this part; ENGINE.md 36.10).
    pub first_picks: Vec<DefId>,
    /// Deck sizes sideboarding may build: 40 (or the first deck's size if
    /// smaller: spec decklists of tests) to `MAX_DECK` (or the first
    /// deck's size if larger).
    pub min_deck: usize,
    pub max_deck: usize,
}

impl MatchPool {
    /// The pool of a player whose first deck is `deck` and sideboard
    /// `sideboard`, and that deck's counts per list entry.
    pub fn new(db: &CardDb, deck: &[DefId], sideboard: &[DefId], first_picks: &[DefId]) -> (MatchPool, Vec<u8>) {
        let basics = basic_ids(db);
        let mut cards: Vec<DefId> = deck.iter().chain(sideboard).copied().filter(|d| !basics.contains(d)).collect();
        cards.sort_unstable();
        let mut list: Vec<(DefId, u8)> = Vec::new();
        for d in cards {
            match list.last_mut() {
                Some((x, n)) if *x == d => *n = n.saturating_add(1),
                _ => list.push((d, 1)),
            }
        }
        let n_cards = list.len();
        list.extend(basics.iter().map(|&b| (b, MAX_COUNT)));
        let mut counts = vec![0u8; list.len()];
        for d in deck {
            let k = list.iter().position(|e| e.0 == *d).expect("a deck card is in the list");
            counts[k] = counts[k].saturating_add(1);
        }
        let mut fp = first_picks.to_vec();
        fp.sort_unstable();
        let pool = MatchPool { list, n_cards, first_picks: fp, min_deck: MIN_DECK.min(deck.len()), max_deck: MAX_DECK.max(deck.len()) };
        (pool, counts)
    }

    /// The deck of these counts (sorted by definition).
    pub fn deck(&self, counts: &[u8]) -> Vec<DefId> {
        let mut v = Vec::with_capacity(40);
        for (k, &(d, _)) in self.list.iter().enumerate() {
            v.extend(std::iter::repeat_n(d, counts[k] as usize));
        }
        v.sort_unstable();
        v
    }

    /// The sideboard of these counts: the pool's cards not in the deck
    /// (sorted; basic lands are not pool cards).
    pub fn sideboard(&self, counts: &[u8]) -> Vec<DefId> {
        let mut v = Vec::new();
        for (k, &(d, n)) in self.list[..self.n_cards].iter().enumerate() {
            v.extend(std::iter::repeat_n(d, n.saturating_sub(counts[k]) as usize));
        }
        v
    }

    /// The counts entry `k` may take (`lo..=hi`, possibly empty) when the
    /// entries before it hold `before` cards and those after it are still
    /// free: a deck of `min_deck..=max_deck` cards must stay reachable (an
    /// entry after `k` may keep its count or take any of `0..=copies`).
    fn range(&self, counts: &[u8], k: usize, before: u32) -> (i64, i64) {
        let rest: u32 = (k + 1..self.list.len()).map(|e| counts[e].max(self.list[e].1.min(MAX_COUNT)) as u32).sum();
        let lo = (self.min_deck as i64 - before as i64 - rest as i64).max(0);
        let hi = (self.list[k].1.min(MAX_COUNT) as i64).min(self.max_deck as i64 - before as i64);
        (lo, hi)
    }
}

/// Sideboarding between two games (ENGINE.md 36.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sideboarding {
    /// Who sideboards first (the next game's chooser), then second.
    pub order: [PlayerId; 2],
    /// The decider: `order[turn]`.
    pub turn: u8,
    /// The decider's deck being built (counts per list entry), and its
    /// deck of the last game.
    pub counts: Vec<u8>,
    pub start: Vec<u8>,
    /// The last entry changed (entries are changed in increasing order).
    pub last: Option<u8>,
    /// A `SideboardCount` for this entry is pending.
    pub asking: Option<u8>,
    /// The decks chosen (the first player's, once done).
    pub done: [Option<Arc<[u8]>>; 2],
    /// The pending decision.
    pub decision: Decision,
}

impl Sideboarding {
    /// Sideboarding with `first` deciding first, from their deck `counts`
    /// (counts per entry of their `pool` list).
    pub fn new(first: PlayerId, counts: Vec<u8>, pool: &MatchPool) -> Sideboarding {
        let mut sb = Sideboarding {
            order: [first, opp(first)],
            turn: 0,
            start: counts.clone(),
            counts,
            last: None,
            asking: None,
            done: [None, None],
            decision: Decision { player: first, kind: DecisionKind::Sideboard, options: Vec::new() },
        };
        sb.ask(pool);
        sb
    }

    pub fn decider(&self) -> PlayerId {
        self.order[self.turn as usize]
    }

    /// The decider answers `a` (`pool`: theirs): `Ok(true)` = done (`PASS`;
    /// `counts` is their deck), else the next decision is asked.
    pub fn choose(&mut self, pool: &MatchPool, a: u16) -> Result<bool, String> {
        let d = &self.decision;
        if d.choice_of(a).is_none() {
            return Err(format!("illegal action {a} for {:?}", d.kind));
        }
        match d.kind {
            DecisionKind::SideboardCount => {
                let k = self.asking.take().expect("an entry") as usize;
                self.counts[k] = (a - act::NUMBER) as u8;
                self.last = Some(k as u8);
            }
            _ if a == act::PASS => return Ok(true),
            _ => {
                let k = (a - act::LIBRARY) as usize;
                if k < pool.n_cards && pool.list[k].1 == 1 {
                    self.counts[k] ^= 1;
                    self.last = Some(k as u8);
                } else {
                    self.asking = Some(k as u8);
                }
            }
        }
        self.ask(pool);
        Ok(false)
    }

    /// The pending decision of the decider's sideboarding (options sorted
    /// by id; never empty: ENGINE.md 36.2).
    pub fn ask(&mut self, pool: &MatchPool) {
        let p = self.decider();
        let counts = &self.counts;
        let mut options = Vec::new();
        if let Some(k) = self.asking {
            let k = k as usize;
            let before: u32 = counts[..k].iter().map(|&c| c as u32).sum();
            let (lo, hi) = pool.range(counts, k, before);
            for x in lo.max(0)..=hi {
                if x != counts[k] as i64 {
                    options.push(Opt { action: act::NUMBER + x as u16, choice: Choice::Number(x as u8) });
                }
            }
            self.decision = Decision { player: p, kind: DecisionKind::SideboardCount, options };
            return;
        }
        let size: usize = counts.iter().map(|&c| c as usize).sum();
        if (pool.min_deck..=pool.max_deck).contains(&size) {
            options.push(Opt { action: act::PASS, choice: Choice::Pass });
        }
        let from = self.last.map_or(0, |l| l as usize + 1);
        let mut before: u32 = counts[..from].iter().map(|&c| c as u32).sum();
        for k in from..pool.list.len() {
            let (lo, hi) = pool.range(counts, k, before);
            let c = counts[k] as i64;
            if hi >= lo && !(lo == hi && lo == c) {
                options.push(Opt { action: act::LIBRARY + k as u16, choice: Choice::Def(pool.list[k].0) });
            }
            before += counts[k] as u32;
        }
        self.decision = Decision { player: p, kind: DecisionKind::Sideboard, options };
    }
}

/// What a match holds besides its current game (`MtgAz::g`).
#[derive(Clone)]
pub struct MatchState {
    pub format: Format,
    /// The settings of every game (the chooser: game 1's).
    pub gspec: GameSpec,
    pub pools: Arc<[MatchPool; 2]>,
    /// Each player's deck in the current (or last) game, counts per list
    /// entry.
    pub decks: [Arc<[u8]>; 2],
    /// The current (or last) game, 1-based.
    pub game_no: u8,
    /// Per game: 0 = not played, 1 + p = player p won, 3 = a draw.
    pub results: [u8; 3],
    /// Per game: who chose who takes the first turn, who took it.
    pub choosers: [PlayerId; 3],
    pub starters: [PlayerId; 3],
    pub wins: [u8; 2],
    pub draws: u8,
    /// Turns of the games before the current one.
    pub turns_before: u32,
    pub over: bool,
    /// Identities (bits) each player revealed in the current game, and in
    /// the earlier games of the match.
    pub seen: [[u64; 16]; 2],
    pub earlier: [[u64; 16]; 2],
    pub sb: Option<Box<Sideboarding>>,
    /// Seeds the later games (reseeded by `determinize`).
    pub rng: Rng,
}

/// The first game's setup of one player: deck, sideboard, first picks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub deck: Vec<DefId>,
    pub sideboard: Vec<DefId>,
    pub first_picks: Vec<DefId>,
}

impl MatchState {
    /// A match whose first game (already made) has these seats.
    pub fn new(db: &CardDb, format: Format, gspec: GameSpec, seats: &[Seat; 2], seed: u64) -> Result<MatchState, String> {
        let (p0, c0) = MatchPool::new(db, &seats[0].deck, &seats[0].sideboard, &seats[0].first_picks);
        let (p1, c1) = MatchPool::new(db, &seats[1].deck, &seats[1].sideboard, &seats[1].first_picks);
        if format == Format::Bo3 {
            for (p, pool) in [&p0, &p1].into_iter().enumerate() {
                if pool.list.len() > act::LIBRARY_N as usize {
                    return Err(format!("player {p}: a pool of {} different cards (at most {})", pool.n_cards, act::LIBRARY_N - 5));
                }
                if let Some(&(d, n)) = pool.list[..pool.n_cards].iter().find(|e| e.1 > MAX_COUNT) {
                    return Err(format!("player {p}: {n} copies of {} (at most {MAX_COUNT})", db.def(d).name));
                }
            }
        }
        Ok(MatchState {
            format,
            gspec,
            pools: Arc::new([p0, p1]),
            decks: [Arc::from(c0), Arc::from(c1)],
            game_no: 1,
            results: [0; 3],
            choosers: [0; 3],
            starters: [0; 3],
            wins: [0; 2],
            draws: 0,
            turns_before: 0,
            over: false,
            seen: [[0; 16]; 2],
            earlier: [[0; 16]; 2],
            sb: None,
            rng: Rng::new(seed ^ 0x3A7C_4B0B_D1CE_5EED),
        })
    }

    /// The deck of player `p` in the current (or last) game.
    pub fn deck(&self, p: PlayerId) -> Vec<DefId> {
        self.pools[p as usize].deck(&self.decks[p as usize])
    }

    /// Has player `p` revealed a card of identity `i` in an earlier game?
    pub fn revealed_before(&self, p: PlayerId, i: u16) -> bool {
        self.earlier[p as usize][i as usize >> 6] & (1 << (i & 63)) != 0
    }

    /// The match's result per seat once over: 1 / 0 to the player with more
    /// game wins, 0.5 each on equal wins.
    pub fn result(&self) -> [f32; 2] {
        match self.wins[0].cmp(&self.wins[1]) {
            std::cmp::Ordering::Greater => [1.0, 0.0],
            std::cmp::Ordering::Less => [0.0, 1.0],
            std::cmp::Ordering::Equal => [0.5, 0.5],
        }
    }
}

impl super::MtgAz {
    /// The sideboarding in progress, if any.
    pub fn sideboarding(&self) -> Option<&Sideboarding> {
        self.m.sb.as_deref()
    }

    /// The pending decision: a sideboarding decision or the game's.
    pub fn pending_decision(&self) -> Option<&Decision> {
        match &self.m.sb {
            Some(sb) => Some(&sb.decision),
            None => self.g.pending_decision(),
        }
    }

    /// Is the match over?
    pub fn match_over(&self) -> bool {
        match self.m.format {
            Format::Bo1 => self.g.is_over(),
            Format::Bo3 => self.m.over,
        }
    }

    /// One action of the match: the game's (and, when it ends the game, the
    /// match's next step: its end or sideboarding) or sideboarding's.
    pub(crate) fn match_step(&mut self, a: u16, chance: &mut Vec<u64>) -> Result<(), String> {
        if self.m.sb.is_some() {
            return self.sideboard_step(a);
        }
        let k0 = chance.len();
        self.g.apply(a, chance)?;
        if self.m.format == Format::Bo3 {
            let ids = identity::identities(self.g.db());
            for &k in &chance[k0..] {
                let d = key_payload(k);
                if key_kind(k) == KEY_REVEAL && d != HIDDEN && (d as usize) < self.g.db().len() {
                    let i = ids[d as usize] as usize;
                    self.m.seen[key_player(k) as usize & 1][i >> 6] |= 1 << (i & 63);
                }
            }
            if self.g.is_over() {
                self.end_game();
            }
        }
        Ok(())
    }

    /// The current game ended (best-of-three): score it; end the match or
    /// start sideboarding.
    fn end_game(&mut self) {
        let m = &mut self.m;
        let o = self.g.outcome().expect("the game is over");
        let i = (m.game_no - 1) as usize;
        let t = self.g.turn();
        m.choosers[i] = t.chooser;
        m.starters[i] = t.starting;
        m.turns_before += t.number;
        match o.winner {
            Some(w) => {
                m.wins[w as usize] += 1;
                m.results[i] = 1 + w;
            }
            None => {
                m.draws += 1;
                m.results[i] = 3;
            }
        }
        for p in 0..2 {
            for w in 0..16 {
                m.earlier[p][w] |= m.seen[p][w];
            }
            m.seen[p] = [0; 16];
        }
        if m.wins.iter().any(|&w| w >= BO3_WINS) || m.game_no >= BO3_GAMES {
            m.over = true;
            return;
        }
        // CR 103.1: the loser of the previous game chooses; after a draw,
        // the player who made the choice in it.
        let chooser = match o.winner {
            Some(w) => opp(w),
            None => t.chooser,
        };
        let sb = Sideboarding::new(chooser, m.decks[chooser as usize].to_vec(), &m.pools[chooser as usize]);
        m.sb = Some(Box::new(sb));
    }

    fn sideboard_step(&mut self, a: u16) -> Result<(), String> {
        let m = &mut self.m;
        let sb = m.sb.as_mut().expect("sideboarding");
        let p = sb.decider();
        if !sb.choose(&m.pools[p as usize], a)? {
            return Ok(());
        }
        sb.done[p as usize] = Some(Arc::from(std::mem::take(&mut sb.counts)));
        if sb.turn == 1 {
            return self.next_game();
        }
        sb.turn = 1;
        let q = sb.decider();
        sb.counts = m.decks[q as usize].to_vec();
        sb.start = sb.counts.clone();
        sb.last = None;
        sb.ask(&m.pools[q as usize]);
        Ok(())
    }

    /// Both players have sideboarded: the next game, its chooser the first
    /// to sideboard (CR 103.1).
    fn next_game(&mut self) -> Result<(), String> {
        let m = &mut self.m;
        let sb = m.sb.take().expect("sideboarding");
        for p in 0..2 {
            m.decks[p] = sb.done[p].clone().expect("both decks chosen");
        }
        m.game_no += 1;
        let decks = [m.deck(0), m.deck(1)];
        let sides = [m.pools[0].sideboard(&m.decks[0]), m.pools[1].sideboard(&m.decks[1])];
        let gspec = GameSpec { chooser: Some(sb.order[0]), ..m.gspec };
        let seed = m.rng.next_u64();
        self.g = Game::with_sideboards(self.g.db(), gspec, [&decks[0], &decks[1]], [&sides[0], &sides[1]], seed)?;
        Ok(())
    }
}
