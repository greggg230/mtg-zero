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
/// own (Sunken Ruins; ENGINE.md 18): such mana never pays such a cost, so
/// no ability pays for itself.
pub const MADE: ManaMask = 1 << 6;

/// A need of a payment that `MADE` units can't pay: the mana cost of a
/// mana ability activated within the payment (CR 605.3a, 602.2b).
pub const NOT_MADE: ManaMask = 1 << 7;

/// Can a unit of `unit` pay a need of `need` (their types meet; a `MADE`
/// unit never pays a `NOT_MADE` need)?
#[inline]
pub fn fits(unit: ManaMask, need: ManaMask) -> bool {
    unit & need & TYPES != 0 && (need & NOT_MADE == 0 || unit & MADE == 0)
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
    pub fn total(&self) -> u8 {
        self.generic
            + self.pips.iter().sum::<u8>()
            + self.hybrid.iter().sum::<u8>()
            + self.phyrexian.iter().sum::<u8>()
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
    /// Phyrexian included).
    pub fn mana_value(&self) -> u8 {
        self.total()
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
    /// engine limit, shortcuts.md 46.4; CR 106 has no maximum).
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

/// Can `cost` (no Phyrexian pips) be paid exactly with every unit in
/// `chosen` plus some of `available`? Each unit makes one mana of a type in
/// its mask.
///
/// Generic pips take any mana, so only the coloured (and hybrid) pips need
/// a matching; among matchings we maximise the chosen units used on pips,
/// and the leftover chosen units must fit into the generic part.
pub fn payable(cost: &ManaCost, chosen: &[ManaMask], available: &[ManaMask]) -> bool {
    payable_iter(cost, &[], chosen, available.iter().copied(), available.len())
}

/// [`payable`] with `extra` needs besides the cost (one mana each, of a
/// type in the mask; `NOT_MADE` ones: the mana costs of mana abilities
/// activated in the payment).
pub fn payable_n(cost: &ManaCost, extra: &[ManaMask], chosen: &[ManaMask], available: &[ManaMask]) -> bool {
    payable_iter(cost, extra, chosen, available.iter().copied(), available.len())
}

/// [`payable`] over any sequence of available masks (no allocation: this
/// runs for every castable card at every priority decision).
fn payable_iter(
    cost: &ManaCost,
    extra: &[ManaMask],
    chosen: &[ManaMask],
    available: impl Iterator<Item = ManaMask>,
    n: usize,
) -> bool {
    let need = cost.total() as usize + extra.len();
    if chosen.len() > need || chosen.len() + n < need {
        return false;
    }
    let mut groups = [(0u8, false, 0u8); 128];
    let mut ng = 0;
    let mut add = |m: ManaMask, ch: bool| match groups[..ng].iter_mut().find(|g| g.0 == m && g.1 == ch) {
        Some(g) => g.2 = g.2.saturating_add(1),
        None => {
            groups[ng] = (m, ch, 1);
            ng += 1;
        }
    };
    for &m in chosen {
        add(m, true);
    }
    for m in available {
        add(m, false);
    }
    let mut needs = [(0u8, 0u8); 24];
    let mut nn = 0;
    for &m in extra {
        match needs[..nn].iter_mut().find(|x| x.0 == m) {
            Some(x) => x.1 += 1,
            None => {
                needs[nn] = (m, 1);
                nn += 1;
            }
        }
    }
    for c in 0..6 {
        if cost.pips[c] > 0 {
            needs[nn] = (1 << c, cost.pips[c]);
            nn += 1;
        }
    }
    for (i, &k) in cost.hybrid.iter().enumerate() {
        if k > 0 {
            needs[nn] = (pair_mask(i), k);
            nn += 1;
        }
    }
    debug_assert!(!cost.has_phyrexian());
    let best = if extra.iter().any(|&m| m & NOT_MADE != 0) {
        best_chosen::<true>(&needs[..nn], &mut groups[..ng], chosen.len() as u8)
    } else {
        best_chosen::<false>(&needs[..nn], &mut groups[..ng], chosen.len() as u8)
    };
    match best {
        Some(used) => chosen.len() - used as usize <= cost.generic as usize,
        None => false,
    }
}

/// The most chosen units the coloured needs can use (stops at `cap`, all of
/// them); `None` if the needs cannot be met.
/// (`STRICT`: some need is `NOT_MADE`; otherwise any meeting of types fits,
/// the fast path.)
fn best_chosen<const STRICT: bool>(needs: &[(ManaMask, u8)], groups: &mut [(ManaMask, bool, u8)], cap: u8) -> Option<u8> {
    match needs.split_first() {
        None => Some(0),
        Some((&(c, k), rest)) => distribute::<STRICT>(c, k, 0, rest, groups, cap),
    }
}

fn distribute<const STRICT: bool>(
    need: ManaMask,
    k: u8,
    gi: usize,
    rest: &[(ManaMask, u8)],
    groups: &mut [(ManaMask, bool, u8)],
    cap: u8,
) -> Option<u8> {
    if k == 0 {
        return best_chosen::<STRICT>(rest, groups, cap);
    }
    if gi == groups.len() {
        return None;
    }
    let (mask, ch, cnt) = groups[gi];
    let meets = if STRICT { fits(mask, need) } else { mask & need != 0 };
    let most = if meets { k.min(cnt) } else { 0 };
    let mut best: Option<u8> = None;
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

/// One mana a source can make, with what using it takes from a shared
/// budget: life (CR 119.4), or a permanent a later part of the cost may
/// need to sacrifice (a Lotus Petal when Tinker must sacrifice an
/// artifact).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Unit {
    pub mask: ManaMask,
    pub life: u8,
    pub sac: bool,
}

impl Unit {
    pub fn free(mask: ManaMask) -> Unit {
        Unit { mask, life: 0, sac: false }
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
    /// Activations of a shared source (Orcish Lumberjack, tapped for each
    /// Forest it sacrifices).
    pub tap: i32,
}

impl Budget {
    /// No limits that matter.
    pub const FREE: Budget = Budget { life: i32::MAX, sac: i32::MAX, tap: i32::MAX };

    fn take(&self, u: &Unit, n: i32) -> Option<Budget> {
        let b = Budget { life: self.life - u.life as i32 * n, sac: self.sac - if u.sac { n } else { 0 }, tap: self.tap };
        (b.life >= 0 && b.sac >= 0).then_some(b)
    }
}

/// [`payable`] within a [`Budget`] for the available units used.
pub fn payable_within(cost: &ManaCost, chosen: &[ManaMask], available: &[Unit], budget: Budget) -> bool {
    payable_within_n(cost, &[], chosen, available, budget)
}

/// [`payable_within`] with `extra` needs (see [`payable_n`]).
pub fn payable_within_n(cost: &ManaCost, extra: &[ManaMask], chosen: &[ManaMask], available: &[Unit], budget: Budget) -> bool {
    if available.iter().try_fold(budget, |b, u| if u.budgeted() { b.take(u, 1) } else { Some(b) }).is_some() {
        return payable_iter(cost, extra, chosen, available.iter().map(|u| u.mask), available.len());
    }
    let free: Vec<ManaMask> = available.iter().filter(|u| !u.budgeted()).map(|u| u.mask).collect();
    let paid: Vec<Unit> = available.iter().copied().filter(|u| u.budgeted()).collect();
    // Rare (low life, discard costs): try how many units of each budgeted
    // group to use.
    let mut groups: Vec<(Unit, u8)> = Vec::new();
    for u in paid {
        match groups.iter_mut().find(|g| g.0 == u) {
            Some(g) => g.1 += 1,
            None => groups.push((u, 1)),
        }
    }
    fn go(cost: &ManaCost, extra: &[ManaMask], chosen: &[ManaMask], units: &mut Vec<ManaMask>, groups: &[(Unit, u8)], b: Budget) -> bool {
        let Some(((u, n), rest)) = groups.split_first() else { return payable_n(cost, extra, chosen, units) };
        for take in (0..=*n).rev() {
            let Some(left) = b.take(u, take as i32) else { continue };
            let len = units.len();
            units.extend(std::iter::repeat_n(u.mask, take as usize));
            let ok = go(cost, extra, chosen, units, rest, left);
            units.truncate(len);
            if ok {
                return true;
            }
        }
        false
    }
    let mut units = free;
    go(cost, extra, chosen, &mut units, &groups, budget)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MU: ManaMask = 1 << U;
    const MW: ManaMask = 1 << W;
    const MR: ManaMask = 1 << R;
    const MB: ManaMask = 1 << B;

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
        assert_eq!(h.colors(), MW | MU | MB);
        assert_eq!(h.to_string(), "{W/U}{B/P}");
        assert!(ManaCost::parse("{2/W}").is_none());
        assert!(ManaCost::parse("").is_none());
    }

    #[test]
    fn payable_respects_colours_and_chosen() {
        let uu = ManaCost::parse("{U}{U}").unwrap();
        assert!(payable(&uu, &[], &[MU, MU]));
        assert!(!payable(&uu, &[], &[MU, MR]));
        assert!(payable(&uu, &[], &[MU, MU | MW]));
        let one_u = ManaCost::parse("{1}{U}").unwrap();
        // A chosen Mountain is fine: it pays the generic.
        assert!(payable(&one_u, &[MR], &[MU]));
        // Two chosen Mountains are not: only one generic pip.
        assert!(!payable(&one_u, &[MR, MR], &[MU]));
        // A chosen dual can go either way.
        assert!(payable(&ManaCost::parse("{U}{W}").unwrap(), &[MU | MW], &[MW]));
        assert!(payable(&ManaCost::parse("{U}{W}").unwrap(), &[MU | MW], &[MU]));
        assert!(!payable(&ManaCost::parse("{U}{W}").unwrap(), &[MU | MW], &[MR]));
    }

    #[test]
    fn hybrid_pips_take_either_colour() {
        let h = ManaCost::parse("{R/W}{R/W}").unwrap();
        assert!(payable(&h, &[], &[MR, MW]));
        assert!(!payable(&h, &[], &[MR, MU]));
        assert!(payable(&h, &[MR], &[MR]));
        let mut pool = ManaPool { amt: [1, 0, 0, 1, 0, 0], art: 0, haste: 0 };
        let mut c = h;
        pool.pay_into(&mut c);
        assert!(c.is_zero());
    }

    #[test]
    fn budgets_limit_life_and_hand_units() {
        let two = ManaCost::parse("{2}").unwrap();
        let b = |life, sac| Budget { life, sac, tap: i32::MAX };
        let conf = Unit { life: 1, ..Unit::free(COLORS) };
        assert!(payable_within(&two, &[], &[conf, conf], b(2, 0)));
        assert!(!payable_within(&two, &[], &[conf, conf], b(1, 0)));
        assert!(payable_within(&two, &[], &[conf, conf, Unit::free(MR)], b(1, 0)));
        let petal = Unit { sac: true, ..Unit::free(COLORS) };
        assert!(!payable_within(&two, &[], &[petal, petal], b(9, 1)));
        assert!(payable_within(&two, &[], &[petal, petal], b(9, 2)));
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
