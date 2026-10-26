//! Card pools for matches (ENGINE.md 36.4), before team drafting exists:
//! seeded random pools of `POOL_SIZE` cards from the supported cards (two
//! main colours, a splash and other off-colour cards, colourless spells,
//! nonbasic lands, sometimes a companion), three of them marked as the
//! player's first picks (data only for now: M8b part b hides them from the
//! opponent), and a heuristic deckbuilder for game 1 (23 spells and 17
//! lands in the pool's best two colours, a sensible curve) whose leftovers
//! are the sideboard (CR 100.4b). Basic lands are not pool cards: a limited
//! deck may add any number of them (CR 100.2b, 100.4b).

use crate::db::{types, CardDb, CardDef};
use crate::ids::DefId;
use crate::mana::COLORS;
use crate::rng::Rng;

use super::decks::{self, DeckGen, BASICS, DECK_SIZE};

/// Cards in a generated pool (a draft's 3 packs of 15).
pub const POOL_SIZE: usize = 45;
/// First picks marked per pool (one per pack).
pub const FIRST_PICKS: usize = 3;
/// The deckbuilder's spells (the rest of the 40 are lands).
pub const SPELLS: usize = 23;

/// A player's pool: its cards (sorted; basic lands are not pool cards) and
/// the first picks among them (sorted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pool {
    pub cards: Vec<DefId>,
    pub first_picks: Vec<DefId>,
}

/// The five basic lands (W, U, B, R, G).
pub fn basic_ids(db: &CardDb) -> [DefId; 5] {
    BASICS.map(|n| db.id(n).expect("basic lands are supported"))
}

/// The deckbuilder's and generator's weight of a spell: creatures 3,
/// interaction 2 (damage, removal, counters, discard, edicts, token
/// makers), other coloured cards 1, colourless ones 0.5 (as `decks`).
fn weight(d: &CardDef) -> f64 {
    match () {
        _ if d.is(types::CREATURE) => 3.0,
        _ if decks::interacts(d) => 2.0,
        _ if d.colors != 0 => 1.0,
        _ => 0.5,
    }
}

/// Can a deck of colours `pair` play spell `d`: its colours within the
/// pair and every coloured pip payable?
fn playable(d: &CardDef, pair: u8) -> bool {
    d.colors & !pair == 0 && decks::castable_with(d, pair)
}

/// Up to `n` weighted picks without replacement from `cands` (weights `w`),
/// skipping cards already in `out`.
fn take(rng: &mut Rng, cands: &[DefId], mut w: Vec<f64>, n: usize, out: &mut Vec<DefId>) {
    for (i, c) in cands.iter().enumerate() {
        if out.contains(c) {
            w[i] = 0.0;
        }
    }
    for _ in 0..n {
        let Some(i) = decks::pick(rng, &w) else { return };
        w[i] = 0.0;
        out.push(cands[i]);
    }
}

/// A random pool of `POOL_SIZE` singleton cards: two main colours; one pool
/// in four a companion its main colours can cast; 27 spells of the main
/// colours (weighted to creatures and interaction), 4 colourless spells, 5
/// nonbasic lands (weighted to the main colours' duals and fetches), then
/// off-colour spells: half of a splash colour's (within the main colours
/// and the splash), half of any colour outside the pair. The first picks
/// are three of its main-colour spells (weighted as the spells).
pub fn random_pool(db: &CardDb, rng: &mut Rng) -> Pool {
    let (lands, spells) = decks::card_pool(db);
    let basics = basic_ids(db);
    let a = rng.below(5) as usize;
    let b = (a + 1 + rng.below(4) as usize) % 5;
    let pair = (1u8 << a) | (1u8 << b);
    let others: Vec<usize> = (0..5).filter(|&c| pair & (1 << c) == 0).collect();
    let splash = 1u8 << others[rng.below(others.len() as u32) as usize];
    let mut cards: Vec<DefId> = Vec::with_capacity(POOL_SIZE);
    let comps: Vec<DefId> = decks::companions(db).into_iter().filter(|&c| decks::castable_with(db.def(c), pair)).collect();
    if rng.below(4) == 0 && !comps.is_empty() {
        cards.push(comps[rng.below(comps.len() as u32) as usize]);
    }
    let spells_of = |f: &dyn Fn(&CardDef) -> bool| -> (Vec<DefId>, Vec<f64>) {
        let c: Vec<DefId> = spells.iter().copied().filter(|&id| f(db.def(id))).collect();
        let w = c.iter().map(|&id| weight(db.def(id))).collect();
        (c, w)
    };
    let (main, w) = spells_of(&|d| d.colors != 0 && d.colors & !pair == 0);
    let n0 = cards.len();
    take(rng, &main, w.clone(), 27, &mut cards);
    let main_picked: Vec<DefId> = cards[n0..].to_vec();
    let (cl, w) = spells_of(&|d| d.colors == 0);
    take(rng, &cl, w, 4, &mut cards);
    let nonbasic: Vec<DefId> = lands.iter().copied().filter(|id| !basics.contains(id)).collect();
    let lw: Vec<f64> = nonbasic
        .iter()
        .map(|&id| {
            let d = db.def(id);
            let m = decks::land_colors(db, d) & COLORS;
            match () {
                _ if m == pair => 4.0,
                _ if m == COLORS => 1.5,
                _ if m & pair == pair => 1.5,
                _ if m & pair != 0 && m & !pair == 0 => 2.0,
                _ if m == 0 && !decks::is_fetch(d) => 1.0,
                _ => 0.3,
            }
        })
        .collect();
    take(rng, &nonbasic, lw, 5, &mut cards);
    let rest = POOL_SIZE - cards.len();
    let (sp, w) = spells_of(&|d| d.colors & splash != 0 && d.colors & !(pair | splash) == 0);
    take(rng, &sp, w, rest / 2, &mut cards);
    let (off, w) = spells_of(&|d| d.colors & !pair != 0);
    take(rng, &off, w, POOL_SIZE - cards.len(), &mut cards);
    let mut firsts = Vec::with_capacity(FIRST_PICKS);
    let fw: Vec<f64> = main_picked.iter().map(|&id| weight(db.def(id))).collect();
    take(rng, &main_picked, fw, FIRST_PICKS, &mut firsts);
    cards.sort_unstable();
    firsts.sort_unstable();
    Pool { cards, first_picks: firsts }
}

/// The heuristic game-1 deckbuilder: a 40-card deck from `pool` plus basic
/// lands, and the sideboard (the pool's other cards, sorted). Deterministic.
///
/// * Colours: the pair whose 23 best playable spells (colours within the
///   pair, every pip payable) weigh most (creatures 3, interaction 2, other
///   coloured cards 1, colourless 0.5; the first pair on a tie).
/// * Spells: the playable ones by weight, then mana value, at most
///   `gen.max_mv5` of mana value 5 or more and `gen.max_mv4` of 4 (the caps
///   lifted if fewer than 23 fit); companions stay in the sideboard (to be
///   revealed when the deck fulfills them).
/// * Lands: the pool's nonbasic lands that make or fetch a colour of the
///   pair and at most one colourless utility land (up to 9), then basics
///   split by the spells' coloured pips (at least a third each), 40 cards
///   in all (more lands when fewer than 23 spells are playable).
pub fn build_deck(db: &CardDb, pool: &[DefId], gen: &DeckGen) -> (Vec<DefId>, Vec<DefId>) {
    let basics = basic_ids(db);
    let spells: Vec<DefId> = pool
        .iter()
        .copied()
        .filter(|&id| !db.def(id).is(types::LAND) && db.def(id).companion.is_none())
        .collect();
    let ranked = |pair: u8| -> Vec<DefId> {
        let mut v: Vec<DefId> = spells.iter().copied().filter(|&id| playable(db.def(id), pair)).collect();
        v.sort_by(|&x, &y| {
            let (dx, dy) = (db.def(x), db.def(y));
            weight(dy).total_cmp(&weight(dx)).then(dx.mana_value().cmp(&dy.mana_value())).then(x.cmp(&y))
        });
        v
    };
    let mut best = (f64::MIN, 0u8);
    for a in 0..5 {
        for b in a + 1..5 {
            let pair = (1u8 << a) | (1u8 << b);
            let score: f64 = ranked(pair).iter().take(SPELLS).map(|&id| weight(db.def(id))).sum();
            if score > best.0 {
                best = (score, pair);
            }
        }
    }
    let pair = best.1;
    let cands = ranked(pair);
    let mut deck: Vec<DefId> = Vec::with_capacity(DECK_SIZE);
    let (mut mv5, mut mv4) = (0u8, 0u8);
    for &id in &cands {
        if deck.len() == SPELLS {
            break;
        }
        let mv = db.def(id).mana_value();
        if mv >= 5 {
            if mv5 >= gen.max_mv5 {
                continue;
            }
            mv5 += 1;
        } else if mv == 4 {
            if mv4 >= gen.max_mv4 {
                continue;
            }
            mv4 += 1;
        }
        deck.push(id);
    }
    for &id in &cands {
        if deck.len() == SPELLS {
            break;
        }
        if !deck.contains(&id) {
            deck.push(id);
        }
    }
    let n_spells = deck.len();
    // Nonbasic lands: on colour first (both colours, then one), then one
    // colourless utility land.
    let mut lands: Vec<(u8, DefId)> = Vec::new();
    for &id in pool {
        let d = db.def(id);
        if !d.is(types::LAND) || basics.contains(&id) {
            continue;
        }
        let m = decks::land_colors(db, d) & COLORS;
        let rank = if m & pair == pair {
            0
        } else if m & pair != 0 {
            1
        } else if m == 0 && !decks::is_fetch(d) {
            2
        } else {
            continue;
        };
        lands.push((rank, id));
    }
    lands.sort_unstable();
    let mut utility = 0;
    for (rank, id) in lands {
        if deck.len() - n_spells >= 9 {
            break;
        }
        if rank == 2 {
            if utility == 1 {
                continue;
            }
            utility += 1;
        }
        deck.push(id);
    }
    // Basics by the spells' coloured pips (at least a third each).
    let mut pips = [0u32; 5];
    for &id in &deck[..n_spells] {
        let c = &db.def(id).cost;
        for (i, p) in pips.iter_mut().enumerate() {
            *p += c.pips[i] as u32 + c.phyrexian[i] as u32;
        }
        for (k, &n) in c.hybrid.iter().enumerate() {
            let m = crate::mana::pair_mask(k);
            for (i, p) in pips.iter_mut().enumerate() {
                if m & (1 << i) != 0 {
                    *p += n as u32;
                }
            }
        }
    }
    let cols: Vec<usize> = (0..5).filter(|&i| pair & (1 << i) != 0).collect();
    let n_basics = DECK_SIZE - deck.len();
    let (pa, pb) = (pips[cols[0]] as f64 + 1.0, pips[cols[1]] as f64 + 1.0);
    let lo = (n_basics as f64 / 3.0).floor() as usize;
    let na = (((n_basics as f64) * pa / (pa + pb)).round() as usize).clamp(lo.min(n_basics), n_basics - lo.min(n_basics));
    for _ in 0..na {
        deck.push(basics[cols[0]]);
    }
    while deck.len() < DECK_SIZE {
        deck.push(basics[cols[1]]);
    }
    deck.sort_unstable();
    let mut side: Vec<DefId> = pool.to_vec();
    for id in &deck {
        if let Some(i) = side.iter().position(|x| x == id) {
            side.remove(i);
        }
    }
    side.sort_unstable();
    (deck, side)
}

/// Two random pools from `rng` (self-play: a match's, from its seed).
pub fn random_pools(db: &CardDb, rng: &mut Rng) -> [Pool; 2] {
    [random_pool(db, rng), random_pool(db, rng)]
}

/// The fixed evaluation pools: pair `i` of the set `seed` (the same for
/// every run and iteration with the same seed; ENGINE.md 36.4).
pub fn eval_pools(db: &CardDb, seed: u64, i: u32) -> [Pool; 2] {
    let mut rng = Rng::new(seed.wrapping_mul(0x2F1E_6A5D_93C7_B481) ^ (0x900_1000 + i as u64));
    random_pools(db, &mut rng)
}
