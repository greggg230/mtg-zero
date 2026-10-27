//! The hidden pool (ENGINE.md 36.11): what a player of a best-of-three match
//! knows of the other player's cards, and what `MtgAz::determinize` samples
//! for the rest.
//!
//! * The viewer knows the opponent's pool but their first picks (one per
//!   pack: the picks no other drafter sees), less the first picks they have
//!   seen: revealed in an earlier game, or known in the current game (on the
//!   battlefield, in a graveyard, looked at in a hand, a revealed companion,
//!   ...). The number of first picks is public.
//! * The opponent's deck (which pool cards, plus how many basic lands of
//!   which type) is hidden within the pool. Public: its size (the cards in
//!   the game), the sideboard's size, so the number of pool cards in the deck
//!   and of basic lands; a revealed companion, so the deck fulfills its
//!   condition.
//!
//! `determinize(viewer)` for the opponent `o`: (1) the unseen first picks
//! are drawn from the prior [`FirstPickPrior`]; (2) the deck is drawn from
//! the pool so sampled, holding every card of `o`'s the viewer knows is in
//! the game and the identities of its clouds, with the public counts, a
//! revealed companion's condition, and weights (colours, cards played
//! before); (3) the deck's cards the viewer does not know are dealt to the
//! cards it does not know (`Game::redeal`), the sideboard is the rest of the
//! pool; then `Game::determinize` shuffles them as before (clouds kept).
//! The viewer's own pool and deck are never resampled. Between games the
//! same is done from the last game's final state, and the first
//! sideboarder's chosen deck, hidden from the second, becomes the sampled
//! last deck (the prior: kept).

use std::sync::Arc;

use crate::db::{types, CardDb, CardDef, DeckCond};
use crate::game::Game;
use crate::ids::{opp, CardIdx, DefId, PlayerId};
use crate::mana::COLORS;
use crate::rng::Rng;

use super::matches::{MatchPool, MatchState, MAX_COUNT};
use super::{decks, identity, pools};

/// Weight factor of a card within the expected colours (first picks and
/// deck cards), against 1 for a card outside them.
const ON_COLOUR: f64 = 4.0;
/// Weight factor of a deck card the opponent revealed in an earlier game.
const PLAYED_BEFORE: f64 = 3.0;

/// The prior of one player's unknown first picks, as the other player
/// samples them (ENGINE.md 36.11): the supported cards (not basic lands,
/// not tokens) that are neither in the known part of the owner's pool nor
/// in the viewer's own pool (the cube is singleton: a card known to be
/// elsewhere is not a first pick), weighted as the generator weighs spells
/// (creatures 3, interaction 2, other coloured cards 1, colourless 0.5) and
/// x4 when playable in `pair`, the colours the deckbuilder would choose for
/// the known part of the pool. Fixed per match.
#[derive(Clone, Debug)]
pub struct FirstPickPrior {
    pub pair: u8,
    pub cands: Vec<DefId>,
    /// Cumulative weights (`cum[i]` = weights of `cands[..=i]`).
    pub cum: Vec<f64>,
}

/// The weight factor of `d` for colours `cols` (lands: by the colours they
/// make or fetch; colourless ones fit any deck).
fn fit(db: &CardDb, d: &CardDef, cols: u8) -> f64 {
    let c = if d.is(types::LAND) { decks::land_colors(db, d) & COLORS } else { d.colors };
    fit_of(d.is(types::LAND), c, cols)
}

fn fit_of(land: bool, c: u8, cols: u8) -> f64 {
    if c == 0 || (if land { c & cols != 0 } else { c & !cols == 0 }) {
        ON_COLOUR
    } else {
        1.0
    }
}

/// The five basic lands of `db` (W U B R G), looked up once.
fn basics_of(db: &'static CardDb) -> [DefId; 5] {
    static B: std::sync::OnceLock<(usize, [DefId; 5])> = std::sync::OnceLock::new();
    let b = B.get_or_init(|| (db as *const CardDb as usize, pools::basic_ids(db)));
    assert_eq!(b.0, db as *const CardDb as usize, "one card database per process");
    b.1
}

/// Per card definition of the cube: (the generator's weight, is a land,
/// its colours for `fit`), computed once (`pools::weight` and
/// `decks::land_colors` read the card's abilities).
fn facts(db: &'static CardDb) -> &'static [(f64, bool, u8)] {
    static FACTS: std::sync::OnceLock<(usize, Vec<(f64, bool, u8)>)> = std::sync::OnceLock::new();
    let f = FACTS.get_or_init(|| {
        let v = (0..db.len())
            .map(|i| {
                let d = db.def(i as DefId);
                let land = d.is(types::LAND);
                let c = if land { decks::land_colors(db, d) & COLORS } else { d.colors };
                (pools::weight(d), land, c)
            })
            .collect();
        (db as *const CardDb as usize, v)
    });
    assert_eq!(f.0, db as *const CardDb as usize, "one card database per process");
    &f.1
}

impl FirstPickPrior {
    /// The prior of `owner`'s first picks (`pools`: both players' real
    /// pools; only what `opp(owner)` knows is read: the known part of
    /// `owner`'s pool and the viewer's own).
    pub fn new(db: &CardDb, pools: &[MatchPool; 2], owner: PlayerId) -> FirstPickPrior {
        let pool = &pools[owner as usize];
        let known: Vec<DefId> = pool.list[..pool.n_cards].iter().map(|e| e.0).filter(|d| pool.first_picks.binary_search(d).is_err()).collect();
        let pair = pools::best_pair(db, &known);
        let mine = &pools[opp(owner) as usize];
        let basics = pools::basic_ids(db);
        let (lands, spells) = decks::card_pool(db);
        let mut cands = Vec::new();
        let mut cum = Vec::new();
        let mut tot = 0.0;
        for id in lands.into_iter().chain(spells) {
            if basics.contains(&id) || known.binary_search(&id).is_ok() || mine.list.iter().any(|e| e.0 == id) {
                continue;
            }
            let d = db.def(id);
            tot += pools::weight(d) * fit(db, d, pair);
            cands.push(id);
            cum.push(tot);
        }
        FirstPickPrior { pair, cands, cum }
    }

    /// `n` distinct first picks, none of them `taken` (sorted) and each
    /// passing `ok`, appended to `out`; false if too few candidates pass.
    fn sample(&self, rng: &mut Rng, n: usize, taken: &[DefId], ok: &dyn Fn(DefId) -> bool, out: &mut Vec<DefId>) -> bool {
        let tot = self.cum.last().copied().unwrap_or(0.0);
        let good = |d: DefId, out: &Vec<DefId>| taken.binary_search(&d).is_err() && !out.contains(&d) && ok(d);
        let start = out.len();
        let mut tries = 0;
        while out.len() - start < n && tries < 64 * n.max(1) {
            tries += 1;
            let x = unit(rng) * tot;
            let i = self.cum.partition_point(|&c| c <= x).min(self.cands.len() - 1);
            let d = self.cands[i];
            if good(d, out) {
                out.push(d);
            }
        }
        if out.len() - start < n {
            // (Rejections: fall back to a weighted pick over what passes.)
            let w: Vec<f64> = (0..self.cands.len())
                .map(|i| if good(self.cands[i], out) { self.cum[i] - if i > 0 { self.cum[i - 1] } else { 0.0 } } else { 0.0 })
                .collect();
            let mut w = w;
            while out.len() - start < n {
                let Some(i) = decks::pick(rng, &w) else { return false };
                w[i] = 0.0;
                out.push(self.cands[i]);
            }
        }
        true
    }
}

fn unit(rng: &mut Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// Remove one `d` from a sorted multiset; false if absent.
fn take_one(v: &mut Vec<DefId>, d: DefId) -> bool {
    match v.binary_search(&d) {
        Ok(i) => {
            v.remove(i);
            true
        }
        Err(_) => false,
    }
}

/// Does the other player know that pool card `d` of `owner`'s is in the
/// pool (before the current game): not a first pick, or a first pick
/// revealed in an earlier game?
pub fn known_before(m: &MatchState, owner: PlayerId, ids: &[u16], d: DefId) -> bool {
    m.pools[owner as usize].first_picks.binary_search(&d).is_err() || m.revealed_before(owner, ids[d as usize])
}

/// What the viewer knows of `owner`'s cards in game `g` (the current, or
/// between games the last).
pub(crate) struct Knowledge {
    /// `owner`'s cards the viewer does not know exactly (dealt again).
    pub slots: Vec<CardIdx>,
    /// The identities (card definitions) of the other cards of `owner`'s
    /// starting deck, sorted.
    pub known: Vec<DefId>,
    /// The identities of non-deck cards among `slots` (a companion put into
    /// the hand and shuffled away, ...): dealt again with the deck's.
    pub extras: Vec<DefId>,
    /// The deck's cards the viewer knows are in it (`known` and the
    /// identities its clouds place among `slots`, less those of `extras`;
    /// the match adds what the owner revealed this game), sorted. (Pinned
    /// cards, kept by `determinize` while their owner pays, count as known:
    /// another viewer than the decider only, ENGINE.md 10; a pinned card in
    /// a cloud's domain counts for its own identity there, as
    /// `Game::cloud_needs` deals it, not twice.)
    pub required: Vec<DefId>,
}

/// The identities (card definitions, sorted) of player `p`'s cards in the
/// current (or last) game of match `a`, wherever they are: their starting
/// deck's and a revealed companion's (tests: in a sampled world they are the
/// sampled deck and the companion; a card the viewer lost track of may have
/// been dealt to any slot the viewer does not know).
pub fn game_cards(a: &super::MtgAz, p: PlayerId) -> Vec<DefId> {
    let sizes = [a.m.decks[0].iter().map(|&c| c as usize).sum(), a.m.decks[1].iter().map(|&c| c as usize).sum()];
    let comp = a.g.players[p as usize].companion;
    let mut v: Vec<DefId> = deck_range(sizes, p).chain(comp.map(|c| c.i())).map(|i| card_of(&a.g, CardIdx(i as u16))).collect();
    v.sort_unstable();
    v
}

/// The card slot `c` is (its own card: not a copy effect's, not a face; a
/// face-down permanent's card): a pool card or a basic land for a deck card.
pub fn card_of(g: &Game, c: CardIdx) -> DefId {
    let d = g.face_down_id(c).unwrap_or_else(|| g.own_def(c));
    g.db().def(d).card_def(d)
}

/// The objects of `p`'s starting deck in a game whose decks have `sizes`
/// cards (`Game::with_sideboards` adds them first, player 0's then 1's).
fn deck_range(sizes: [usize; 2], p: PlayerId) -> std::ops::Range<usize> {
    if p == 0 {
        0..sizes[0]
    } else {
        sizes[0]..sizes[0] + sizes[1]
    }
}

pub(crate) fn knowledge(g: &Game, viewer: PlayerId, owner: PlayerId, sizes: [usize; 2]) -> Knowledge {
    let db = g.db();
    let (slots, pinned) = g.hidden_slots(viewer, owner);
    let range = deck_range(sizes, owner);
    let mut in_slots = vec![false; g.objs.len()];
    for &c in &slots {
        in_slots[c.i()] = true;
    }
    let mut known = Vec::with_capacity(range.len());
    for i in range.clone() {
        if !in_slots[i] {
            known.push(card_of(g, CardIdx(i as u16)));
        }
    }
    known.sort_unstable();
    // (A revealed companion is not a deck card: dealt again with the deck's
    // only when the viewer has lost track of it, say shuffled into the
    // library after the special action.)
    let mut extras: Vec<DefId> = slots.iter().filter(|c| !range.contains(&c.i())).map(|&c| card_of(g, c)).collect();
    extras.sort_unstable();
    let mut required = known.clone();
    for need in g.cloud_needs(viewer, owner, &slots, &pinned).1 {
        required.extend(need.iter().map(|&d| db.def(d).card_def(d)));
    }
    required.sort_unstable();
    for &x in &extras {
        take_one(&mut required, x);
    }
    Knowledge { slots: slots.to_vec(), known, extras, required }
}

/// May card `d` be in a deck that fulfills companion condition `cond`
/// (per card; `DifferentNames` limits copies, below)?
fn allowed(db: &CardDb, cond: Option<&DeckCond>, d: DefId) -> bool {
    match cond {
        Some(DeckCond::Each { cards, must }) => db.def_matches(cards, d) != Some(true) || db.def_matches(must, d) == Some(true),
        _ => true,
    }
}

fn one_per_name(db: &CardDb, cond: Option<&DeckCond>, d: DefId) -> bool {
    matches!(cond, Some(DeckCond::DifferentNames { cards }) if db.def_matches(cards, d) == Some(true))
}

/// The sampled pool of `owner` for the viewer: the known part, the seen
/// first picks and `drawn` (new first picks), with the real pool's deck
/// size (40 for a pool's deck, ENGINE.md 42: sideboarding in the world keeps
/// it, as the real one does).
fn pool_with(real: &MatchPool, keep: &[DefId], seen_fp: &[DefId], drawn: &[DefId]) -> MatchPool {
    let mut cards: Vec<(DefId, u8)> = real.list[..real.n_cards].iter().copied().filter(|e| keep.binary_search(&e.0).is_ok()).collect();
    cards.extend(seen_fp.iter().chain(drawn).map(|&d| (d, 1)));
    cards.sort_unstable();
    let n_cards = cards.len();
    cards.extend_from_slice(&real.list[real.n_cards..]);
    let mut first_picks: Vec<DefId> = seen_fp.iter().chain(drawn).copied().collect();
    first_picks.sort_unstable();
    MatchPool { list: cards, n_cards, first_picks, min_deck: real.min_deck, max_deck: real.max_deck }
}

/// A deck of `n` cards, `m` of them pool cards (the rest basic lands), from
/// `pool`'s list holding `required` (sorted definitions), fulfilling
/// companion `comp`'s condition (that card stays in the sideboard), drawn
/// by weight: on colour (`cols`) x4, played in an earlier game x3. Counts
/// per list entry, or `None` if impossible.
#[allow(clippy::too_many_arguments)]
fn sample_deck(
    db: &'static CardDb,
    m: &MatchState,
    owner: PlayerId,
    ids: &[u16],
    pool: &MatchPool,
    required: &[DefId],
    n: usize,
    npool: usize,
    comp: Option<DefId>,
    cols: u8,
    rng: &mut Rng,
) -> Option<Vec<u8>> {
    let cond = comp.and_then(|c| db.def(c).companion.as_ref());
    let mut counts = vec![0u8; pool.list.len()];
    for &d in required {
        let k = match pool.list[..pool.n_cards].binary_search_by_key(&d, |e| e.0) {
            Ok(k) => k,
            Err(_) => pool.n_cards + pool.list[pool.n_cards..].iter().position(|e| e.0 == d)?,
        };
        counts[k] += 1;
        if counts[k] > pool.list[k].1 {
            return None;
        }
    }
    let have: usize = counts[..pool.n_cards].iter().map(|&c| c as usize).sum();
    let basics_have: usize = counts[pool.n_cards..].iter().map(|&c| c as usize).sum();
    if have > npool || n < npool || basics_have > n - npool {
        return None;
    }
    // Pool cards, by weight per copy.
    let mut avail = vec![0u8; pool.n_cards];
    let mut w = vec![0.0f64; pool.n_cards];
    for k in 0..pool.n_cards {
        let (d, copies) = pool.list[k];
        let mut a = copies.saturating_sub(counts[k]);
        if Some(d) == comp {
            a = a.saturating_sub(1);
        }
        if !allowed(db, cond, d) {
            a = 0;
        }
        if one_per_name(db, cond, d) {
            a = a.min(1u8.saturating_sub(counts[k]));
        }
        avail[k] = a;
        let (wt, land, c) = facts(db)[d as usize];
        w[k] = wt * fit_of(land, c, cols) * if m.revealed_before(owner, ids[d as usize]) { PLAYED_BEFORE } else { 1.0 };
    }
    // (Weighted picks without replacement, the total kept as it goes.)
    let mut ws: Vec<f64> = (0..pool.n_cards).map(|k| w[k] * avail[k] as f64).collect();
    let mut tot: f64 = ws.iter().sum();
    for _ in have..npool {
        if tot <= 1e-9 {
            return None;
        }
        let mut x = unit(rng) * tot;
        let mut k = usize::MAX;
        for (i, &v) in ws.iter().enumerate() {
            if v > 0.0 {
                k = i;
                if x < v {
                    break;
                }
                x -= v;
            }
        }
        if k == usize::MAX {
            return None;
        }
        counts[k] += 1;
        avail[k] -= 1;
        let nw = w[k] * avail[k] as f64;
        tot += nw - ws[k];
        ws[k] = nw;
    }
    // Basic lands: the rest, of the deck's colours (or any).
    let basic_cols: Vec<usize> = (0..5).filter(|&i| cols & (1 << i) != 0 && allowed(db, cond, pool.list[pool.n_cards + i].0)).collect();
    let basic_cols: Vec<usize> = if basic_cols.is_empty() { (0..5).collect() } else { basic_cols };
    // A type at `MAX_COUNT` takes no more: the rest goes to the deck's other
    // colours, then to any type (runs/mtg4 stalled on a 60-card deck of 34
    // basic lands in one colour pair; a 40-card deck's basics always fit in
    // two colours, but a spec decklist of tests keeps its size).
    for _ in basics_have..n - npool {
        let open = |i: &usize| counts[pool.n_cards + *i] < MAX_COUNT;
        let mut choice: Vec<usize> = basic_cols.iter().copied().filter(open).collect();
        if choice.is_empty() {
            choice = (0..5).filter(|i| allowed(db, cond, pool.list[pool.n_cards + i].0)).filter(open).collect();
        }
        if choice.is_empty() {
            return None;
        }
        let i = choice[rng.below(choice.len() as u32) as usize];
        counts[pool.n_cards + i] += 1;
    }
    Some(counts)
}

impl super::MtgAz {
    /// `determinize(viewer)` of a best-of-three match (ENGINE.md 36.11):
    /// the opponent's unknown first picks, deck and sideboard sampled from
    /// what `viewer` knows, then the game's hidden cards dealt from that
    /// deck (`Game::determinize` follows; not between games). The viewer's
    /// own pool, deck and sideboard are never resampled.
    pub(crate) fn sample_hidden_pool(&mut self, viewer: PlayerId, rng: &mut Rng) {
        let o = opp(viewer);
        let db = self.g.db();
        let ids = identity::identities(db);
        let m = &self.m;
        let Some(priors) = m.priors.clone() else { return };
        let prior = &priors[o as usize];
        let sizes = [m.decks[0].iter().map(|&c| c as usize).sum(), m.decks[1].iter().map(|&c| c as usize).sum()];
        let mut kn = knowledge(&self.g, viewer, o, sizes);
        let real = &m.pools[o as usize];
        let comp = self.g.players[o as usize].companion_def;
        // Cards they revealed in this (between games: the last) game are in
        // the deck, wherever they are now (a revealed companion is not).
        for &(d, _) in &real.list {
            let i = ids[d as usize] as usize;
            if m.seen[o as usize][i >> 6] & (1 << (i & 63)) != 0 && Some(d) != comp {
                if let Err(at) = kn.required.binary_search(&d) {
                    kn.required.insert(at, d);
                }
            }
        }
        // (1) First picks: the seen ones stay; the others are drawn.
        let seen_fp: Vec<DefId> = real
            .first_picks
            .iter()
            .copied()
            .filter(|&d| m.revealed_before(o, ids[d as usize]) || kn.required.binary_search(&d).is_ok() || Some(d) == comp || kn.extras.contains(&d))
            .collect();
        let keep: Vec<DefId> = real.list[..real.n_cards].iter().map(|e| e.0).filter(|d| real.first_picks.binary_search(d).is_err()).collect();
        let unseen = real.first_picks.len() - seen_fp.len();
        let n = sizes[o as usize];
        let pool_cards: usize = real.list[..real.n_cards].iter().map(|e| e.1 as usize).sum();
        let npool = pool_cards - self.g.players[o as usize].sideboard.len();
        // The deck's colours: the prior's pair, and those of the spells and
        // basic lands the viewer knows are in it.
        let basics = basics_of(db);
        let mut cols = prior.pair;
        for &d in &kn.required {
            match basics.iter().position(|&b| b == d) {
                Some(i) => cols |= 1 << i,
                None if !db.def(d).is(types::LAND) => cols |= db.def(d).colors & COLORS,
                None => {}
            }
        }
        let cond = comp.and_then(|c| db.def(c).companion.as_ref());
        let mut taken: Vec<DefId> = Vec::with_capacity(keep.len() + seen_fp.len());
        taken.extend_from_slice(&keep);
        for &d in &seen_fp {
            let at = taken.partition_point(|&x| x < d);
            taken.insert(at, d);
        }
        let mut attempt = 0;
        let (pool, counts) = loop {
            // (Retry once with first picks that may be in the companion's
            // deck, then without the companion's condition: never needed
            // when the sampler's assumptions hold, ENGINE.md 36.11.)
            let ok_cond = attempt == 1;
            let ok = |d: DefId| !ok_cond || allowed(db, cond, d);
            let mut drawn = Vec::with_capacity(unseen);
            assert!(prior.sample(rng, unseen, &taken, &ok, &mut drawn), "too few first-pick candidates");
            let pool = pool_with(real, &keep, &seen_fp, &drawn);
            let c = if attempt < 2 { comp } else { None };
            if let Some(counts) = sample_deck(db, m, o, ids, &pool, &kn.required, n, npool, c, cols, rng) {
                break (pool, counts);
            }
            attempt += 1;
            assert!(attempt < 3, "no deck of {n} cards ({npool} from the pool) holds what P{viewer} knows of P{o}'s");
        };
        // (3) The deck's cards the viewer does not know, to its unknown
        // cards; the sideboard: the rest of the pool.
        let mut deal: Vec<DefId> = pool.deck(&counts);
        deal.extend_from_slice(&kn.extras);
        deal.sort_unstable();
        for &d in &kn.known {
            assert!(take_one(&mut deal, d), "a card P{viewer} knows of P{o}'s is not in the sampled deck");
        }
        assert_eq!(deal.len(), kn.slots.len(), "P{o}'s sampled deck does not fit their cards");
        let side: Arc<[DefId]> = Arc::from(pool.sideboard(&counts));
        if self.m.sb.is_none() {
            self.g.redeal(o, &kn.slots, &deal, side);
        }
        let counts: Arc<[u8]> = Arc::from(counts);
        let mut pools = (*self.m.pools).clone();
        pools[o as usize] = pool;
        self.m.pools = Arc::new(pools);
        self.m.decks[o as usize] = counts.clone();
        // Between games: the first sideboarder's chosen deck is hidden from
        // the second (prior: kept); the other player's sideboarding in
        // progress is theirs (restarted from the sampled deck).
        if let Some(sb) = self.m.sb.as_mut() {
            if sb.done[o as usize].is_some() {
                sb.done[o as usize] = Some(counts.clone());
            }
            if sb.decider() == o {
                sb.counts = counts.to_vec();
                sb.start = sb.counts.clone();
                sb.last = None;
                sb.asking = None;
                sb.ask(&self.m.pools[o as usize]);
            }
        }
    }
}
