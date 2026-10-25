//! Decks for self-play and evaluation (ENGINE.md 21.3): legal 40-card decks
//! from the supported cards only, two colours, 16-17 lands with the
//! supported duals and fetch lands, a sensible curve; seeded.

use crate::db::{types, ActEffect, CardDb, CardDef, Filter, ManaProd};
use crate::ids::DefId;
use crate::mana::COLORS;
use crate::program::Op;
use crate::rng::Rng;

/// Deck size (limited).
pub const DECK_SIZE: usize = 40;
/// The basic land of each colour (W, U, B, R, G).
pub const BASICS: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];

/// Settings of the generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeckGen {
    /// Lands per deck, drawn uniformly from `min_lands..=max_lands`.
    pub min_lands: u8,
    pub max_lands: u8,
    /// At most this many spells of mana value 5 or more, and of 4.
    pub max_mv5: u8,
    pub max_mv4: u8,
}

impl Default for DeckGen {
    fn default() -> Self {
        DeckGen { min_lands: 16, max_lands: 17, max_mv5: 3, max_mv4: 4 }
    }
}

/// The colours a land can make (a `ManaMask`, colourless bit included) or,
/// for a fetch land, find (the basic land types its search names; any
/// colour for "a basic land").
pub fn land_colors(db: &CardDb, d: &CardDef) -> u8 {
    let mut m = 0u8;
    for a in &d.activated {
        match &a.effect {
            ActEffect::Mana(ma) => {
                m |= match &ma.prod {
                    ManaProd::One(mask) => *mask,
                    ManaProd::Fixed(amt) => (0..6).filter(|&i| amt[i] > 0).fold(0, |m, i| m | 1 << i),
                    ManaProd::SameType { mask, .. } | ManaProd::Combo { mask, .. } => *mask,
                    ManaProd::PerPermanent { ty, .. } => 1 << *ty,
                    ManaProd::ColorAmong(_) => COLORS,
                    ManaProd::IfElse { then, els, .. } => (0..6).filter(|&i| then[i] + els[i] > 0).fold(0, |m, i| m | 1 << i),
                    // (A summary for deck building.)
                    ManaProd::OneOf(_) | ManaProd::Linked | ManaProd::SacrificedLand | ManaProd::Chosen | ManaProd::IfCounter { .. } => {
                        crate::lower::activated::prod_mask(&ma.prod)
                    }
                }
            }
            ActEffect::Stack(ab) => {
                for op in &ab.program {
                    if let Op::Search { filter, .. } = op {
                        m |= search_colors(db, filter);
                    }
                }
            }
        }
    }
    m
}

/// Colours of the basic land types (or "basic") a search filter names.
fn search_colors(db: &CardDb, f: &Filter) -> u8 {
    match f {
        Filter::Subtype(s) => {
            BASICS.iter().position(|b| db.subtype_name(*s) == *b).map_or(0, |i| 1 << i)
        }
        Filter::Supertype(s) if s & crate::db::supertypes::BASIC != 0 => COLORS,
        Filter::And(a, b) => search_colors(db, a) | search_colors(db, b),
        Filter::Or(v) => v.iter().fold(0, |m, x| m | search_colors(db, x)),
        _ => 0,
    }
}

/// A fetch land: a land without mana abilities that searches for lands.
fn is_fetch(d: &CardDef) -> bool {
    d.activated.iter().all(|a| a.mana().is_none())
        && d.activated.iter().any(|a| match &a.effect {
            ActEffect::Stack(ab) => ab.program.iter().any(|op| matches!(op, Op::Search { .. })),
            ActEffect::Mana(_) => false,
        })
}

/// Does the card interact (damage, removal, counters, discard)?
fn interacts(d: &CardDef) -> bool {
    let prog = |ops: &[Op]| {
        ops.iter().any(|op| {
            matches!(
                op,
                Op::DealDamage { .. }
                    | Op::DamageAll { .. }
                    | Op::DamagePlayer { .. }
                    | Op::Destroy { .. }
                    | Op::Exile { .. }
                    | Op::Counter { .. }
                    | Op::Sacrifice { .. }
                    | Op::ChooseDiscard { .. }
                    | Op::DiscardRandom { .. }
                    | Op::CreateToken { .. }
            )
        })
    };
    let ab = |a: &crate::db::AbilityDef| prog(&a.program) || a.modes.as_ref().is_some_and(|m| m.list.iter().any(|x| prog(&x.program)));
    d.spell.as_ref().is_some_and(ab)
        || d.triggers.iter().any(|t| ab(&t.ability))
        || d.activated.iter().any(|a| matches!(&a.effect, ActEffect::Stack(x) if ab(x)))
}

/// The supported non-token cards: (lands, spells).
fn card_pool(db: &CardDb) -> (Vec<DefId>, Vec<DefId>) {
    let (mut lands, mut spells) = (Vec::new(), Vec::new());
    for id in 0..db.num_cards() as DefId {
        let d = db.def(id);
        if d.token {
            continue;
        }
        if d.is(types::LAND) {
            lands.push(id);
        } else {
            spells.push(id);
        }
    }
    (lands, spells)
}

/// Weighted pick without replacement; returns the index into `w`.
fn pick(rng: &mut Rng, w: &[f64]) -> Option<usize> {
    let tot: f64 = w.iter().sum();
    if tot <= 0.0 {
        return None;
    }
    let mut x = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * tot;
    for (i, &v) in w.iter().enumerate() {
        if v > 0.0 {
            if x < v {
                return Some(i);
            }
            x -= v;
        }
    }
    w.iter().rposition(|&v| v > 0.0)
}

/// A random legal 40-card deck of supported cards: two colours chosen at
/// random; 23-24 singleton spells of those colours or colourless (weighted
/// to creatures and interaction, at most `max_mv5` of mana value 5+ and
/// `max_mv4` of 4); 16-17 lands: every supported land that makes or fetches
/// exactly these two colours, some lands that make or fetch one of them or
/// any colour and some colourless utility lands (singleton), then basics in
/// proportion to the spells' coloured pips.
pub fn generate(db: &CardDb, rng: &mut Rng, gen: &DeckGen) -> Vec<DefId> {
    let a = rng.below(5) as usize;
    let b = (a + 1 + rng.below(4) as usize) % 5;
    generate_colors(db, rng, gen, (1 << a) | (1 << b))
}

/// [`generate`] for the colours of `pair` (a `ManaMask` of two colours).
pub fn generate_colors(db: &CardDb, rng: &mut Rng, gen: &DeckGen, pair: u8) -> Vec<DefId> {
    let (lands, spells) = card_pool(db);
    let span = (gen.max_lands - gen.min_lands) as u32 + 1;
    let n_lands = gen.min_lands as usize + rng.below(span) as usize;
    let n_spells = DECK_SIZE - n_lands;
    // Spells.
    let cands: Vec<DefId> = spells.iter().copied().filter(|&id| db.def(id).colors & !pair == 0).collect();
    let mut w: Vec<f64> = cands
        .iter()
        .map(|&id| {
            let d = db.def(id);
            let on = d.colors != 0;
            match () {
                _ if d.is(types::CREATURE) => 3.0,
                _ if interacts(d) => 2.0,
                _ if on => 1.0,
                _ => 0.5,
            }
        })
        .collect();
    let mut deck: Vec<DefId> = Vec::with_capacity(DECK_SIZE);
    let (mut mv5, mut mv4) = (0u8, 0u8);
    while deck.len() < n_spells {
        let Some(i) = pick(rng, &w) else { break };
        w[i] = 0.0;
        let d = db.def(cands[i]);
        let mv = d.mana_value();
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
        deck.push(cands[i]);
    }
    // Nonbasic lands (singleton).
    let basic_ids: Vec<DefId> = BASICS.iter().map(|n| db.id(n).expect("basic lands are supported")).collect();
    let mut nonbasic = Vec::new();
    for &id in &lands {
        if basic_ids.contains(&id) {
            continue;
        }
        let d = db.def(id);
        let m = land_colors(db, d) & COLORS;
        let p = if m == pair {
            0.9
        } else if m == COLORS {
            0.3
        } else if m & pair != 0 && m & !pair == 0 {
            0.5 // one of the colours (Karakas; a fetch half on colour)
        } else if m & pair == pair {
            0.4 // a triome (or wider) with both colours
        } else if m == 0 && !is_fetch(d) {
            0.15 // colourless utility (Ancient Tomb, Wasteland, Library of Alexandria)
        } else {
            0.0
        };
        if p > 0.0 && rng.below(1000) < (p * 1000.0) as u32 {
            nonbasic.push(id);
        }
    }
    rng.shuffle(&mut nonbasic);
    nonbasic.truncate(n_lands.saturating_sub(7).min(9));
    deck.extend_from_slice(&nonbasic);
    // Basics in proportion to the coloured pips (at least a third each).
    let mut pips = [0u32; 5];
    for &id in &deck[..deck.len() - nonbasic.len()] {
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
    let mut na = ((n_basics as f64) * pa / (pa + pb)).round() as usize;
    na = na.clamp(lo.min(n_basics), n_basics - lo.min(n_basics));
    for _ in 0..na {
        deck.push(basic_ids[cols[0]]);
    }
    while deck.len() < DECK_SIZE {
        deck.push(basic_ids[cols[1]]);
    }
    deck.sort_unstable();
    deck
}

/// The fixed evaluation deck set: pair `i` of `n` (two decks, colours of
/// both chosen at random), the same for every run and iteration with the
/// same `seed`.
pub fn eval_pair(db: &CardDb, gen: &DeckGen, seed: u64, i: u32) -> [Vec<DefId>; 2] {
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (0xE7A1 + i as u64));
    [generate(db, &mut rng, gen), generate(db, &mut rng, gen)]
}

/// Check a deck: 40 cards, supported non-token cards only, at most one copy
/// of each card but basic lands, `min_lands..=max_lands` lands, colours of
/// the spells within two.
pub fn check(db: &CardDb, deck: &[DefId], gen: &DeckGen) -> Result<(), String> {
    if deck.len() != DECK_SIZE {
        return Err(format!("{} cards", deck.len()));
    }
    let mut colors = 0u8;
    let mut lands = 0;
    for (i, &id) in deck.iter().enumerate() {
        if id as usize >= db.num_cards() {
            return Err(format!("definition {id} is not a card"));
        }
        let d = db.def(id);
        let basic = d.supertypes & crate::db::supertypes::BASIC != 0;
        if !basic && deck[..i].contains(&id) {
            return Err(format!("{} twice", d.name));
        }
        if d.is(types::LAND) {
            lands += 1;
        } else {
            colors |= d.colors;
        }
    }
    if lands < gen.min_lands as usize || lands > gen.max_lands as usize {
        return Err(format!("{lands} lands"));
    }
    if colors.count_ones() > 2 {
        return Err(format!("spells of colours {colors:05b}"));
    }
    Ok(())
}
