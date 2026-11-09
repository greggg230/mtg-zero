//! Mana: types, costs, pools and payment feasibility (CR 106, 107.4, 202,
//! 601.2g-h; ENGINE.md 6).

/// Mana types in index order: white, blue, black, red, green, colourless.
pub const W: usize = 0;
pub const U: usize = 1;
pub const B: usize = 2;
pub const R: usize = 3;
pub const G: usize = 4;
pub const C: usize = 5;
pub const LETTERS: [char; 6] = ['W', 'U', 'B', 'R', 'G', 'C'];

/// Bit i set = mana type i. A colour set uses only the WUBRG bits.
pub type ManaMask = u8;

/// The five colours.
pub const COLORS: ManaMask = 0b1_1111;

/// The six mana types.
pub const TYPES: ManaMask = 0b11_1111;

/// A unit made in this payment by a mana ability with a mana cost of its
/// own (Sunken Ruins; ENGINE.md 18), its tag's chain id the ability's
/// (`chain`): it pays such a cost only of an ability activated after its
/// own (`fits`), so no ability pays for itself.
pub const MADE: ManaMask = 1 << 6;

/// A need of a payment that a `MADE` unit pays only when its ability was
/// activated first: the mana cost of a mana ability activated within the
/// payment (CR 605.3a, 602.2b), its tag's chain id that ability's.
pub const NOT_MADE: ManaMask = 1 << 7;

/// Where a tag's chain id starts (`chain`).
const CHAIN_SHIFT: u32 = 16;

/// The chain id of a tag (`Tag` bits 16-63): of a `MADE` unit, the mana
/// ability with a mana cost that made it; of a `NOT_MADE` need, the one
/// whose cost it is (ENGINE.md 100.4.7, docs/shortcuts.md 13.15; 0 for
/// none). Ids name the payment's activations of such abilities
/// (`pay::chain_id`); `payable_iter` tries their orders, relabelling the
/// ids by rank.
#[inline]
pub fn chain(t: Tag) -> u64 {
    t >> CHAIN_SHIFT
}

/// `t` with chain id `id`.
#[inline]
pub fn with_chain(t: Tag, id: u64) -> Tag {
    (t & ((1 << CHAIN_SHIFT) - 1)) | id << CHAIN_SHIFT
}

/// On a unit (never on a need, where the bit is `NOT_MADE`): mana with a
/// spending restriction (CR 106.6: Mishra's Workshop's "only to cast
/// artifact spells"), which can't pay Wizard's Rockets' {X}, so the Rockets
/// never turns it into colours (ENGINE.md 49). Which purposes it may pay is
/// the unit's tag (`Tag`), not this bit.
pub const RESTRICTED: ManaMask = 1 << 7;

/// A unit's or a need's tag in the feasibility searches (ENGINE.md 100.4.5,
/// docs/shortcuts.md N21): bits 0-7 a `ManaMask` (the types, `MADE`, and
/// `NOT_MADE` on a need or `RESTRICTED` on a unit), bits 8-15 spending
/// purposes (`purposes`), bits 16-63 a chain id (`chain`). On a unit they
/// are its restriction (CR 106.6: the `SPEND_*` purposes of its mana, 0 for
/// none; `FOR_COST` for a creature tapped for convoke); on a need, what
/// paying it is (the payment's `Keep::allow` and `FOR_COST` for the cost
/// being paid, `Game::ability_allow` of the ability for a mana ability's own
/// cost).
pub type Tag = u64;

/// A purpose of no mana ability: paying the cost the payment is for (the
/// spell's or the activated ability's), never the mana cost of a mana
/// ability activated within it. A creature tapped for convoke pays only it
/// (CR 702.51a: it pays for the spell's mana). (The purpose byte's top bit:
/// `db::MANA_UNCOUNTERABLE` is a rider, never a purpose, `restriction`.)
pub const FOR_COST: u8 = 0x80;

/// `mask` with spending purposes `purposes` (`Tag`).
#[inline]
pub fn tag(mask: ManaMask, purposes: u8) -> Tag {
    mask as Tag | (purposes as Tag) << 8
}

/// A tag's spending purposes.
#[inline]
pub fn purposes(t: Tag) -> u8 {
    (t >> 8) as u8
}

/// A tag's `ManaMask` (its types and flags, without the purposes).
#[inline]
pub fn low(t: Tag) -> ManaMask {
    t as u8
}

/// The purposes a mana ability's `only` restricts its mana to, as a unit's
/// tag bits (its rider, "that spell can't be countered", is no purpose).
#[inline]
pub fn restriction(only: u8) -> Tag {
    ((only & !crate::db::MANA_UNCOUNTERABLE) as Tag) << 8
}

/// May the unit tagged `unit` be spent on the need tagged `need` (CR 106.6:
/// it has no restriction, or one of its purposes is the need's)?
#[inline]
pub fn spend_ok(unit: Tag, need: Tag) -> bool {
    let only = purposes(unit);
    only == 0 || only & purposes(need) != 0
}

/// Can a unit tagged `unit` pay a need tagged `need` (their types meet; a
/// `MADE` unit pays a `NOT_MADE` need only of an ability after its own in
/// the order the chain ids rank, `chain`; its restriction allows the need's
/// purposes)?
#[inline]
pub fn fits(unit: Tag, need: Tag) -> bool {
    unit & need & TYPES as Tag != 0
        && (need & NOT_MADE as Tag == 0 || unit & MADE as Tag == 0 || chain(unit) != 0 && chain(unit) < chain(need))
        && spend_ok(unit, need)
}

/// Two-colour hybrid pairs (CR 107.4e) in index order.
pub const PAIRS: [(usize, usize); 10] =
    [(W, U), (W, B), (W, R), (W, G), (U, B), (U, R), (U, G), (B, R), (B, G), (R, G)];

pub fn pair_index(a: usize, b: usize) -> Option<usize> {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    PAIRS.iter().position(|&p| p == (a, b))
}

pub fn pair_mask(i: usize) -> ManaMask {
    (1 << PAIRS[i].0) | (1 << PAIRS[i].1)
}

pub fn letter_index(s: &str) -> Option<usize> {
    match s {
        "W" => Some(W),
        "U" => Some(U),
        "B" => Some(B),
        "R" => Some(R),
        "G" => Some(G),
        "C" => Some(C),
        _ => None,
    }
}

/// A mana cost without X (CR 107.3; not supported yet, the lowering
/// reports it). `pips[C]` counts `{C}`; hybrid pips are counted per colour
/// pair (`PAIRS`), Phyrexian pips per colour (CR 107.4f).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct ManaCost {
    pub generic: u8,
    pub pips: [u8; 6],
    pub hybrid: [u8; 10],
    pub phyrexian: [u8; 5],
}

impl ManaCost {
    /// Every symbol, counted in u32 (exact: the u8 sum wrapped past 255,
    /// and a cost of 256 read as none; ENGINE.md 100.4.6).
    pub fn total(&self) -> u32 {
        let sum = |v: &[u8]| v.iter().map(|&n| n as u32).sum::<u32>();
        self.generic as u32 + sum(&self.pips) + sum(&self.hybrid) + sum(&self.phyrexian)
    }

    pub fn is_zero(&self) -> bool {
        self.total() == 0
    }

    /// The two costs together (an additional cost's mana, kicker).
    pub fn plus(&self, o: &ManaCost) -> ManaCost {
        let mut c = *self;
        c.generic = c.generic.saturating_add(o.generic);
        for i in 0..6 {
            c.pips[i] = c.pips[i].saturating_add(o.pips[i]);
        }
        for i in 0..10 {
            c.hybrid[i] = c.hybrid[i].saturating_add(o.hybrid[i]);
        }
        for i in 0..5 {
            c.phyrexian[i] = c.phyrexian[i].saturating_add(o.phyrexian[i]);
        }
        c
    }

    /// This cost with its generic part reduced by up to n.
    pub fn less_generic(&self, n: u8) -> ManaCost {
        let mut c = *self;
        c.generic = c.generic.saturating_sub(n);
        c
    }

    /// Mana value (CR 202.3): every symbol counts one (hybrid and
    /// Phyrexian included). (Of a printed cost: a u8, the lowering's bounds.)
    pub fn mana_value(&self) -> u8 {
        u8::try_from(self.total()).expect("a printed cost's mana value: a u8")
    }

    pub fn has_phyrexian(&self) -> bool {
        self.phyrexian.iter().any(|&n| n > 0)
    }

    /// The colours of a card with this cost (CR 202.2).
    pub fn colors(&self) -> ManaMask {
        let mut m = (0..5).filter(|&i| self.pips[i] > 0 || self.phyrexian[i] > 0).fold(0, |m, i| m | (1 << i));
        for (i, &n) in self.hybrid.iter().enumerate() {
            if n > 0 {
                m |= pair_mask(i);
            }
        }
        m
    }

    /// Parse `{3}`, `{1}{U}`, `{U}{U}`, `{C}`, `{W/U}`, `{B/P}`; anything
    /// else (X, `{2/W}`) is `None`.
    pub fn parse(s: &str) -> Option<ManaCost> {
        let mut c = ManaCost::default();
        let mut rest = s.trim();
        if rest.is_empty() {
            return None;
        }
        while !rest.is_empty() {
            let inner = rest.strip_prefix('{')?;
            let end = inner.find('}')?;
            let sym = &inner[..end];
            if let Ok(n) = sym.parse::<u8>() {
                c.generic = c.generic.checked_add(n)?;
            } else if let Some((a, b)) = sym.split_once('/') {
                let x = letter_index(a).filter(|&x| x < C)?;
                if b == "P" {
                    c.phyrexian[x] += 1;
                } else {
                    let y = letter_index(b).filter(|&y| y < C && y != x)?;
                    c.hybrid[pair_index(x, y)?] += 1;
                }
            } else {
                c.pips[letter_index(sym)?] += 1;
            }
            rest = &inner[end + 1..];
        }
        Some(c)
    }
}

impl std::fmt::Display for ManaCost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.generic > 0 || self.is_zero() {
            write!(f, "{{{}}}", self.generic)?;
        }
        for (i, &n) in self.hybrid.iter().enumerate() {
            for _ in 0..n {
                write!(f, "{{{}/{}}}", LETTERS[PAIRS[i].0], LETTERS[PAIRS[i].1])?;
            }
        }
        for (i, &n) in self.phyrexian.iter().enumerate() {
            for _ in 0..n {
                write!(f, "{{{}/P}}", LETTERS[i])?;
            }
        }
        for (i, &n) in self.pips.iter().enumerate() {
            for _ in 0..n {
                write!(f, "{{{}}}", LETTERS[i])?;
            }
        }
        Ok(())
    }
}

/// A player's mana pool (CR 106.4). Empties at the end of each step (500.4).
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct ManaPool {
    pub amt: [u8; 6],
    /// The part of `amt[C]` that may be spent only to cast artifact spells
    /// (Mishra's Workshop's mana left over, CR 106.6; ENGINE.md 28).
    pub art: u8,
    /// The part of `amt[R]` that gives haste to the creature spell it is
    /// spent on (Arena of Glory; ENGINE.md 31).
    pub haste: u8,
}

impl ManaPool {
    /// All the pool's mana (summed wide: six types of up to 255).
    pub fn total(&self) -> u32 {
        self.amt.iter().map(|&n| n as u32).sum()
    }

    pub fn clear(&mut self) {
        self.amt = [0; 6];
        self.art = 0;
        self.haste = 0;
    }

    /// [`pay_into`](Self::pay_into) for a cost that artifact-only mana may
    /// pay (`art_ok`: casting an artifact spell), spending it first;
    /// otherwise that mana stays.
    pub fn pay_into_for(&mut self, cost: &mut ManaCost, art_ok: bool) {
        if self.art == 0 {
            return self.pay_any(cost);
        }
        if art_ok {
            let c0 = self.amt[C];
            self.pay_any(cost);
            self.art = self.art.saturating_sub(c0 - self.amt[C]);
        } else {
            let held = self.art;
            self.amt[C] -= held;
            self.art = 0;
            self.pay_any(cost);
            self.amt[C] += held;
            self.art = held;
        }
    }

    /// Spend pool mana that has no spending restriction (see
    /// [`pay_into_for`](Self::pay_into_for)).
    pub fn pay_into(&mut self, cost: &mut ManaCost) {
        self.pay_into_for(cost, false)
    }

    /// Add mana (CR 106.4). At most 255 of a type: more saturates (an
    /// engine limit, shortcuts.md 46.3; CR 106 has no maximum).
    pub fn add(&mut self, mana: &[u8; 6]) {
        for (a, &n) in self.amt.iter_mut().zip(mana) {
            *a = a.saturating_add(n);
        }
    }

    /// Spend pool mana on `cost` (no Phyrexian pips) as far as it goes,
    /// most constrained first: single-type pips from their type, hybrid
    /// pips from the more plentiful of their two colours, then generic
    /// (colourless first, then the most plentiful colour). Using the pool
    /// first is never worse: it empties at the end of the step anyway
    /// (ENGINE.md 5). Any of the pool's mana.
    fn pay_any(&mut self, cost: &mut ManaCost) {
        for c in 0..6 {
            let t = self.amt[c].min(cost.pips[c]);
            self.amt[c] -= t;
            cost.pips[c] -= t;
        }
        for (h, &(a, b)) in cost.hybrid.iter_mut().zip(PAIRS.iter()) {
            while *h > 0 && (self.amt[a] > 0 || self.amt[b] > 0) {
                let x = if self.amt[a] >= self.amt[b] { a } else { b };
                self.amt[x] -= 1;
                *h -= 1;
            }
        }
        while cost.generic > 0 {
            let pick = if self.amt[C] > 0 {
                Some(C)
            } else {
                (0..5).filter(|&c| self.amt[c] > 0).max_by_key(|&c| (self.amt[c], 5 - c))
            };
            match pick {
                Some(c) => {
                    self.amt[c] -= 1;
                    cost.generic -= 1;
                }
                None => break,
            }
        }
    }
}

/// Can `cost` (no Phyrexian pips), paid for purposes `allow` (the payment's
/// `Keep::allow`), be paid exactly with every unit in `chosen` plus some of
/// `available`? Each unit makes one mana of a type in its tag, spendable as
/// its tag's restriction says (`Tag`).
///
/// Generic pips take any mana, so only the coloured (and hybrid) pips need
/// a matching; among matchings we maximise the chosen units used on pips,
/// and the leftover chosen units must fit into the generic part. When a
/// unit's restriction keeps it from some need (CR 106.6: Mishra's
/// Workshop's {C} and Chromatic Star's {1}; Sunken Citadel's mana, which
/// pays only a land's ability), the generic pips are needs as well and the
/// answer is an exact matching (`payable_restricted`).
pub fn payable(cost: &ManaCost, allow: u8, chosen: &[Tag], available: &[Tag]) -> bool {
    payable_iter(cost, allow, &[], chosen, available.iter().copied(), available.len())
}

/// [`payable`] with `extra` needs besides the cost (one mana each, of a
/// type in the tag; `NOT_MADE` ones: the mana costs of mana abilities
/// activated in the payment, with the purposes paying them is).
pub fn payable_n(cost: &ManaCost, allow: u8, extra: &[Tag], chosen: &[Tag], available: &[Tag]) -> bool {
    payable_iter(cost, allow, extra, chosen, available.iter().copied(), available.len())
}

/// The tag of the needs of a cost paid for purposes `allow` (its pips with
/// `FOR_COST`, `Tag`).
#[inline]
pub fn cost_need(mask: ManaMask, allow: u8) -> Tag {
    tag(mask, allow | FOR_COST)
}

/// [`payable`] over any sequence of available tags (no allocation: this
/// runs for every castable card at every priority decision).
///
/// With mana abilities that have a mana cost (`NOT_MADE` needs among
/// `extra`) and the mana of such abilities (`MADE` units) that could pay one
/// of another's (CR 605.3a, 602.2b: a mana ability's mana pays the cost of
/// one activated after it, never its own or an earlier one's), the payment
/// is exact when some order of those abilities allows it: each order is
/// tried, its chain ids relabelled by rank (`chain`, `fits`), abilities alike
/// (the same needs and units) in a fixed order among themselves
/// (docs/shortcuts.md 13.15: no such mana paid such a cost).
fn payable_iter(
    cost: &ManaCost,
    allow: u8,
    extra: &[Tag],
    chosen: &[Tag],
    available: impl Iterator<Item = Tag> + Clone,
    n: usize,
) -> bool {
    let need = cost.total() as usize + extra.len();
    if chosen.len() > need || chosen.len() + n < need {
        return false;
    }
    let made = |u: &Tag| u & MADE as Tag != 0 && chain(*u) != 0;
    let linked = extra.iter().any(|&e| e & NOT_MADE as Tag != 0 && chain(e) != 0 && chosen.iter().copied().chain(available.clone()).any(|u| made(&u) && chain(u) != chain(e) && u & e & TYPES as Tag != 0 && spend_ok(u, e)));
    if !linked {
        return payable_ordered(cost, allow, extra, chosen, available);
    }
    // The ids, and each one's needs and units without it (alike when equal).
    let units: Vec<Tag> = chosen.iter().copied().chain(available).collect();
    let mut ids: Vec<u64> = extra.iter().filter(|&&e| e & NOT_MADE as Tag != 0).chain(units.iter().filter(|u| made(u))).map(|&t| chain(t)).filter(|&i| i != 0).collect();
    ids.sort_unstable();
    ids.dedup();
    let sig = |id: u64| {
        let strip = |t: &Tag| with_chain(*t, 0);
        let mut ns: Vec<Tag> = extra.iter().filter(|&&e| e & NOT_MADE as Tag != 0 && chain(e) == id).map(strip).collect();
        let mut us: Vec<Tag> = units.iter().filter(|u| made(u) && chain(**u) == id).map(strip).collect();
        ns.sort_unstable();
        us.sort_unstable();
        (ns, us)
    };
    let sigs: Vec<(Vec<Tag>, Vec<Tag>)> = ids.iter().map(|&i| sig(i)).collect();
    let mut nodes = SEARCH_NODES;
    // (Each order: rank r for the r-th id placed; an id alike an earlier one
    // only after it.)
    #[allow(clippy::too_many_arguments)]
    fn orders(
        k: usize,
        rank: &mut Vec<u64>,
        placed: &mut Vec<bool>,
        sigs: &[(Vec<Tag>, Vec<Tag>)],
        ids: &[u64],
        try_one: &mut dyn FnMut(&[u64]) -> bool,
        nodes: &mut u32,
    ) -> Option<bool> {
        *nodes = nodes.checked_sub(1)?;
        if k == ids.len() {
            return Some(try_one(rank));
        }
        for i in 0..ids.len() {
            if placed[i] || (0..i).any(|j| !placed[j] && sigs[j] == sigs[i]) {
                continue;
            }
            placed[i] = true;
            rank[i] = k as u64 + 1;
            let r = orders(k + 1, rank, placed, sigs, ids, try_one, nodes);
            placed[i] = false;
            if r != Some(false) {
                return r;
            }
        }
        Some(false)
    }
    let relabel = |t: Tag, rank: &[u64]| match ids.binary_search(&chain(t)) {
        Ok(i) if chain(t) != 0 => with_chain(t, rank[i]),
        _ => t,
    };
    let mut try_one = |rank: &[u64]| {
        let ex: Vec<Tag> = extra.iter().map(|&t| relabel(t, rank)).collect();
        let ch: Vec<Tag> = chosen.iter().map(|&t| relabel(t, rank)).collect();
        let av: Vec<Tag> = units[chosen.len()..].iter().map(|&t| relabel(t, rank)).collect();
        payable_ordered(cost, allow, &ex, &ch, av.iter().copied())
    };
    let mut rank = vec![0u64; ids.len()];
    let mut placed = vec![false; ids.len()];
    orders(0, &mut rank, &mut placed, &sigs, &ids, &mut try_one, &mut nodes)
        .unwrap_or_else(|| search_exhausted(format_args!("the orders of chained mana abilities: cost {cost}, needs {extra:?}, units {units:?}")))
}

/// [`payable_iter`] with the chained abilities' order given (their chain
/// ids' ranks).
fn payable_ordered(cost: &ManaCost, allow: u8, extra: &[Tag], chosen: &[Tag], available: impl Iterator<Item = Tag> + Clone) -> bool {
    // (A restriction that keeps some unit from some need: the exact
    // matching. Otherwise every unit may pay every need it meets in type.)
    let main = cost_need(0, allow);
    let kept_from = |u: Tag| purposes(u) != 0 && (!spend_ok(u, main) || extra.iter().any(|&e| !spend_ok(u, e)));
    if chosen.iter().any(|&u| kept_from(u)) || available.clone().any(kept_from) {
        return payable_restricted(cost, main, extra, chosen, available);
    }
    // (Counted in u32: any number of units of a tag, never saturating.)
    let mut groups: crate::svec::SVec<(Tag, bool, u32), 32> = crate::svec::SVec::new();
    let mut add = |m: Tag, ch: bool| match groups.iter_mut().find(|g| g.0 == m && g.1 == ch) {
        Some(g) => g.2 += 1,
        None => groups.push((m, ch, 1)),
    };
    for &m in chosen {
        add(m, true);
    }
    for m in available {
        add(m, false);
    }
    let mut needs: crate::svec::SVec<(Tag, u32), 24> = crate::svec::SVec::new();
    for &m in extra {
        match needs.iter_mut().find(|x| x.0 == m) {
            Some(x) => x.1 += 1,
            None => needs.push((m, 1)),
        }
    }
    for c in 0..6 {
        if cost.pips[c] > 0 {
            needs.push((cost_need(1 << c, allow), cost.pips[c] as u32));
        }
    }
    for (i, &k) in cost.hybrid.iter().enumerate() {
        if k > 0 {
            needs.push((cost_need(pair_mask(i), allow), k as u32));
        }
    }
    debug_assert!(!cost.has_phyrexian());
    let cap = chosen.len() as u32;
    let best = if extra.iter().any(|&m| m & NOT_MADE as Tag != 0) {
        best_chosen::<true>(&needs, &mut groups, cap)
    } else {
        best_chosen::<false>(&needs, &mut groups, cap)
    };
    match best {
        Some(used) => chosen.len() - used as usize <= cost.generic as usize,
        None => false,
    }
}

/// [`payable_iter`] when spending restrictions matter (ENGINE.md 100.4.5):
/// the needs (the cost's pips, its generic pips, which any unit its
/// restriction lets pay the cost may pay, a typeless improvise unit too, and
/// `extra`) on one side, the units on the other. The payment is exact when
/// a matching covers every need and every chosen unit; by the
/// Mendelsohn-Dulmage theorem one exists when a matching covers the needs
/// and another covers the chosen units, so two maximum flows decide it (the
/// sides grouped by tag, capacities their counts).
fn payable_restricted(cost: &ManaCost, main: Tag, extra: &[Tag], chosen: &[Tag], available: impl Iterator<Item = Tag>) -> bool {
    // Needs by tag: (tag, generic, count).
    let mut needs: Vec<(Tag, bool, u32)> = Vec::new();
    let mut add_need = |t: Tag, generic: bool, k: u32| {
        if k == 0 {
            return;
        }
        match needs.iter_mut().find(|x| x.0 == t && x.1 == generic) {
            Some(x) => x.2 += k,
            None => needs.push((t, generic, k)),
        }
    };
    for &e in extra {
        add_need(e, false, 1);
    }
    for c in 0..6 {
        add_need(main | (1 << c), false, cost.pips[c] as u32);
    }
    for (i, &k) in cost.hybrid.iter().enumerate() {
        add_need(main | pair_mask(i) as Tag, false, k as u32);
    }
    add_need(main, true, cost.generic as u32);
    // Units by tag: (tag, chosen, count).
    let mut units: Vec<(Tag, bool, u32)> = Vec::new();
    let mut add_unit = |t: Tag, ch: bool| match units.iter_mut().find(|x| x.0 == t && x.1 == ch) {
        Some(x) => x.2 += 1,
        None => units.push((t, ch, 1)),
    };
    for &u in chosen {
        add_unit(u, true);
    }
    for u in available {
        add_unit(u, false);
    }
    let pays = |u: Tag, n: &(Tag, bool, u32)| if n.1 { spend_ok(u, n.0) } else { fits(u, n.0) };
    let edges: Vec<Vec<bool>> = needs.iter().map(|n| units.iter().map(|u| pays(u.0, n)).collect()).collect();
    let need_total: u32 = needs.iter().map(|n| n.2).sum();
    let chosen_total = chosen.len() as u32;
    // Every need paid (by chosen or available units).
    let left: Vec<u32> = needs.iter().map(|n| n.2).collect();
    let right: Vec<u32> = units.iter().map(|u| u.2).collect();
    if max_flow(&left, &right, &edges) < need_total {
        return false;
    }
    // Every chosen unit spent on a need.
    let right_ch: Vec<u32> = units.iter().map(|u| if u.1 { u.2 } else { 0 }).collect();
    max_flow(&left, &right_ch, &edges) == chosen_total
}

/// The maximum flow from the left nodes (capacity `left[i]` each) to the
/// right nodes (capacity `right[j]`) along the edges (`edges[i][j]`, of any
/// capacity): a bipartite b-matching, by augmenting paths.
pub(crate) fn max_flow(left: &[u32], right: &[u32], edges: &[Vec<bool>]) -> u32 {
    let (nl, nr) = (left.len(), right.len());
    let mut f = vec![vec![0u32; nr]; nl];
    let mut out_l = vec![0u32; nl];
    let mut in_r = vec![0u32; nr];
    // An augmenting path from left node i with residual capacity: right
    // nodes reached through edges, back to left nodes through flow.
    fn augment(i: usize, left: &[u32], right: &[u32], edges: &[Vec<bool>], f: &mut [Vec<u32>], in_r: &mut [u32], seen_l: &mut [bool], seen_r: &mut [bool]) -> bool {
        let _ = left;
        for j in 0..right.len() {
            if !edges[i][j] || seen_r[j] {
                continue;
            }
            seen_r[j] = true;
            if in_r[j] < right[j] {
                f[i][j] += 1;
                in_r[j] += 1;
                return true;
            }
            for k in 0..f.len() {
                if f[k][j] > 0 && !seen_l[k] {
                    seen_l[k] = true;
                    if augment(k, left, right, edges, f, in_r, seen_l, seen_r) {
                        f[k][j] -= 1;
                        f[i][j] += 1;
                        return true;
                    }
                }
            }
        }
        false
    }
    let mut total = 0;
    for i in 0..nl {
        while out_l[i] < left[i] {
            let mut seen_l = vec![false; nl];
            let mut seen_r = vec![false; nr];
            seen_l[i] = true;
            if !augment(i, left, right, edges, &mut f, &mut in_r, &mut seen_l, &mut seen_r) {
                break;
            }
            out_l[i] += 1;
            total += 1;
        }
    }
    total
}

/// The most chosen units the coloured needs can use (stops at `cap`, all of
/// them); `None` if the needs cannot be met.
/// (`STRICT`: some need is `NOT_MADE`; otherwise any meeting of types fits,
/// the fast path.)
fn best_chosen<const STRICT: bool>(needs: &[(Tag, u32)], groups: &mut [(Tag, bool, u32)], cap: u32) -> Option<u32> {
    match needs.split_first() {
        None => Some(0),
        Some((&(c, k), rest)) => distribute::<STRICT>(c, k, 0, rest, groups, cap),
    }
}

fn distribute<const STRICT: bool>(
    need: Tag,
    k: u32,
    gi: usize,
    rest: &[(Tag, u32)],
    groups: &mut [(Tag, bool, u32)],
    cap: u32,
) -> Option<u32> {
    if k == 0 {
        return best_chosen::<STRICT>(rest, groups, cap);
    }
    if gi == groups.len() {
        return None;
    }
    let (mask, ch, cnt) = groups[gi];
    let meets = if STRICT { fits(mask, need) } else { mask & need & TYPES as Tag != 0 };
    let most = if meets { k.min(cnt) } else { 0 };
    let mut best: Option<u32> = None;
    for t in (0..=most).rev() {
        let used = if ch { t } else { 0 };
        groups[gi].2 -= t;
        let r = distribute::<STRICT>(need, k - t, gi + 1, rest, groups, cap.saturating_sub(used));
        groups[gi].2 += t;
        if let Some(v) = r {
            let v = v + used;
            best = Some(best.map_or(v, |b| b.max(v)));
            if v >= cap {
                break; // every chosen unit is used: nothing better exists
            }
        }
    }
    best
}

// One mana a source can make, with what using it takes from a shared
/// budget: life (CR 119.4), or a permanent a later part of the cost may
/// need to sacrifice (a Lotus Petal when Tinker must sacrifice an
/// artifact).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Unit {
    pub mask: Tag,
    pub life: u8,
    pub sac: bool,
}

impl Unit {
    pub fn free(mask: impl Into<Tag>) -> Unit {
        Unit { mask: mask.into(), life: 0, sac: false }
    }
    fn budgeted(&self) -> bool {
        self.life > 0 || self.sac
    }
}

/// How much life and how many sacrifice candidates the units of a payment
/// may use.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Budget {
    pub life: i32,
    pub sac: i32,
}

impl Budget {
    /// No limits that matter. (The actors of shared activations, Gene
    /// Pollinators and Orcish Lumberjacks, are counted per pool by the
    /// payment's search: `pay::PoolCtx`, ENGINE.md 100.4.7.)
    pub const FREE: Budget = Budget { life: i32::MAX, sac: i32::MAX };

    fn take(&self, u: &Unit, n: i32) -> Option<Budget> {
        // (In i64: exact for any number of units; what is left is within
        // the i32 it was taken from.)
        let life = self.life as i64 - u.life as i64 * n as i64;
        let sac = self.sac as i64 - if u.sac { n as i64 } else { 0 };
        (life >= 0 && sac >= 0).then_some(Budget { life: life as i32, sac: sac as i32 })
    }
}

/// [`payable`] within a [`Budget`] for the available units used.
pub fn payable_within(cost: &ManaCost, allow: u8, chosen: &[Tag], available: &[Unit], budget: Budget) -> bool {
    payable_within_n(cost, allow, &[], chosen, available, budget)
}

/// [`payable_within`] with `extra` needs (see [`payable_n`]).
pub fn payable_within_n(cost: &ManaCost, allow: u8, extra: &[Tag], chosen: &[Tag], available: &[Unit], budget: Budget) -> bool {
    if available.iter().try_fold(budget, |b, u| if u.budgeted() { b.take(u, 1) } else { Some(b) }).is_some() {
        return payable_iter(cost, allow, extra, chosen, available.iter().map(|u| u.mask), available.len());
    }
    let free: Vec<Tag> = available.iter().filter(|u| !u.budgeted()).map(|u| u.mask).collect();
    let paid: Vec<Unit> = available.iter().copied().filter(|u| u.budgeted()).collect();
    // Rare (low life, discard costs): try how many units of each budgeted
    // group to use.
    // (Counted in u32: any number of units, never wrapping.)
    let mut groups: Vec<(Unit, u32)> = Vec::new();
    for u in paid {
        match groups.iter_mut().find(|g| g.0 == u) {
            Some(g) => g.1 += 1,
            None => groups.push((u, 1)),
        }
    }
    // (`None`: the search's nodes ran out.)
    #[allow(clippy::too_many_arguments)]
    fn go(
        cost: &ManaCost,
        allow: u8,
        extra: &[Tag],
        chosen: &[Tag],
        units: &mut Vec<Tag>,
        groups: &[(Unit, u32)],
        b: Budget,
        nodes: &mut u32,
    ) -> Option<bool> {
        *nodes = nodes.checked_sub(1)?;
        let Some(((u, n), rest)) = groups.split_first() else { return Some(payable_n(cost, allow, extra, chosen, units)) };
        for take in (0..=*n).rev() {
            let Some(left) = b.take(u, take.min(i32::MAX as u32) as i32) else { continue };
            let len = units.len();
            units.extend(std::iter::repeat_n(u.mask, take as usize));
            let ok = go(cost, allow, extra, chosen, units, rest, left, nodes);
            units.truncate(len);
            if ok? {
                return Some(true);
            }
        }
        Some(false)
    }
    let mut units = free;
    let mut nodes = SEARCH_NODES;
    go(cost, allow, extra, chosen, &mut units, &groups, budget, &mut nodes).unwrap_or_else(|| {
        search_exhausted(format_args!(
            "units within a budget: cost {cost}, needs {extra:?}, chosen {chosen:?}, units {available:?}, budget {budget:?}"
        ))
    })
}

/// The most nodes one search of a payment's feasibility visits
/// (`payable_within_n`'s budgeted units, `pay::ways_payable` over the cards
/// with several ways, the virtual pool's spends in `most_spent_from`).
/// These searches are exponential in what a board can hold; the most one
/// visited in the fuzz's games and matches is far below this (ENGINE.md
/// 51). Past it the search stops with a panic that prints its inputs (that
/// search's reproduction, run alone), never with an answer it did not find:
/// a search that does not end hangs a self-play run's worker until the
/// stall watchdog kills the run (fuzz game 10049522).
pub(crate) const SEARCH_NODES: u32 = 1 << 24;

/// A feasibility search ran out of its `SEARCH_NODES`: a loud failure.
#[cold]
#[inline(never)]
pub(crate) fn search_exhausted(what: std::fmt::Arguments) -> ! {
    panic!("pay: a feasibility search visited {SEARCH_NODES} nodes without an answer (ENGINE.md 51): {what}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MU: Tag = 1 << U;
    const MW: Tag = 1 << W;
    const MR: Tag = 1 << R;
    const MB: Tag = 1 << B;

    #[test]
    fn parse_and_display() {
        let c = ManaCost::parse("{1}{U}{U}").unwrap();
        assert_eq!(c.generic, 1);
        assert_eq!(c.pips[U], 2);
        assert_eq!(c.to_string(), "{1}{U}{U}");
        assert_eq!(ManaCost::parse("{3}").unwrap().total(), 3);
        assert!(ManaCost::parse("{X}").is_none());
        let h = ManaCost::parse("{W/U}{B/P}").unwrap();
        assert_eq!(h.hybrid[pair_index(W, U).unwrap()], 1);
        assert_eq!(h.phyrexian[B], 1);
        assert_eq!(h.mana_value(), 2);
        assert_eq!(h.colors() as Tag, MW | MU | MB);
        assert_eq!(h.to_string(), "{W/U}{B/P}");
        assert!(ManaCost::parse("{2/W}").is_none());
        assert!(ManaCost::parse("").is_none());
    }

    #[test]
    fn payable_respects_colours_and_chosen() {
        let uu = ManaCost::parse("{U}{U}").unwrap();
        assert!(payable(&uu, 0, &[], &[MU, MU]));
        assert!(!payable(&uu, 0, &[], &[MU, MR]));
        assert!(payable(&uu, 0, &[], &[MU, MU | MW]));
        let one_u = ManaCost::parse("{1}{U}").unwrap();
        // A chosen Mountain is fine: it pays the generic.
        assert!(payable(&one_u, 0, &[MR], &[MU]));
        // Two chosen Mountains are not: only one generic pip.
        assert!(!payable(&one_u, 0, &[MR, MR], &[MU]));
        // A chosen dual can go either way.
        assert!(payable(&ManaCost::parse("{U}{W}").unwrap(), 0, &[MU | MW], &[MW]));
        assert!(payable(&ManaCost::parse("{U}{W}").unwrap(), 0, &[MU | MW], &[MU]));
        assert!(!payable(&ManaCost::parse("{U}{W}").unwrap(), 0, &[MU | MW], &[MR]));
    }

    #[test]
    fn hybrid_pips_take_either_colour() {
        let h = ManaCost::parse("{R/W}{R/W}").unwrap();
        assert!(payable(&h, 0, &[], &[MR, MW]));
        assert!(!payable(&h, 0, &[], &[MR, MU]));
        assert!(payable(&h, 0, &[MR], &[MR]));
        let mut pool = ManaPool { amt: [1, 0, 0, 1, 0, 0], art: 0, haste: 0 };
        let mut c = h;
        pool.pay_into(&mut c);
        assert!(c.is_zero());
    }

    #[test]
    fn budgets_limit_life_and_hand_units() {
        let two = ManaCost::parse("{2}").unwrap();
        let b = |life, sac| Budget { life, sac };
        let conf = Unit { life: 1, ..Unit::free(COLORS) };
        assert!(payable_within(&two, 0, &[], &[conf, conf], b(2, 0)));
        assert!(!payable_within(&two, 0, &[], &[conf, conf], b(1, 0)));
        assert!(payable_within(&two, 0, &[], &[conf, conf, Unit::free(MR)], b(1, 0)));
        let petal = Unit { sac: true, ..Unit::free(COLORS) };
        assert!(!payable_within(&two, 0, &[], &[petal, petal], b(9, 1)));
        assert!(payable_within(&two, 0, &[], &[petal, petal], b(9, 2)));
    }

    /// CR 106.6 (ENGINE.md 100.4.5): a restricted unit pays only needs whose
    /// purposes its restriction allows, a chosen one too; generic pips of
    /// the cost are needs then (`payable_restricted`).
    #[test]
    fn restricted_units_pay_only_their_purposes() {
        let land = tag(1 << U, crate::db::SPEND_LAND_ABILITY);
        let need_land = tag(TYPES | NOT_MADE, crate::db::SPEND_LAND_ABILITY);
        let need_other = tag(TYPES | NOT_MADE, 0);
        let one = ManaCost::parse("{1}").unwrap();
        assert!(payable_n(&one, 0, &[need_land], &[land], &[MR]), "the land's need, the Mountain the {{1}}");
        assert!(!payable_n(&one, 0, &[need_other], &[land], &[MR]), "not another ability's need");
        assert!(!payable_n(&one, 0, &[], &[land], &[MR]), "not the cost's generic");
        assert!(!payable_n(&one, 0, &[need_land], &[], &[land, land]), "nothing pays the {{1}}");
        assert!(payable_n(&one, 0, &[need_land, need_land], &[MR], &[land, land]));
        assert!(payable_n(&one, crate::db::SPEND_LAND_ABILITY, &[], &[land], &[]), "a land's ability's own cost");
    }

    #[test]
    fn pool_pays_first() {
        let mut pool = ManaPool { amt: [0, 1, 2, 0, 0, 0], art: 0, haste: 0 };
        let mut cost = ManaCost::parse("{1}{U}").unwrap();
        pool.pay_into(&mut cost);
        assert!(cost.is_zero());
        assert_eq!(pool.amt[B], 1);
    }
}
