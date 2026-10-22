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

/// A mana cost without X, hybrid or Phyrexian symbols (those are not
/// supported yet; the lowering reports them). `pips[C]` counts `{C}`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct ManaCost {
    pub generic: u8,
    pub pips: [u8; 6],
}

impl ManaCost {
    pub fn total(&self) -> u8 {
        self.generic + self.pips.iter().sum::<u8>()
    }

    pub fn is_zero(&self) -> bool {
        self.total() == 0
    }

    /// Mana value (CR 202.3).
    pub fn mana_value(&self) -> u8 {
        self.total()
    }

    /// The colours of a card with this cost (CR 202.2).
    pub fn colors(&self) -> ManaMask {
        (0..5).filter(|&i| self.pips[i] > 0).fold(0, |m, i| m | (1 << i))
    }

    /// Parse `{3}`, `{1}{U}`, `{U}{U}`, `{C}`; anything else is `None`.
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
}

impl ManaPool {
    pub fn total(&self) -> u8 {
        self.amt.iter().sum()
    }

    pub fn clear(&mut self) {
        self.amt = [0; 6];
    }

    /// Spend pool mana on `cost` as far as it goes: coloured pips from their
    /// colour, then generic (colourless first, then the most plentiful
    /// colour). Using the pool first is never worse: it empties at the end of
    /// the step anyway (ENGINE.md 5).
    pub fn pay_into(&mut self, cost: &mut ManaCost) {
        for c in 0..6 {
            let t = self.amt[c].min(cost.pips[c]);
            self.amt[c] -= t;
            cost.pips[c] -= t;
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

/// Can `cost` be paid exactly with every source in `chosen` plus some of
/// `available`? Each source makes one mana of a type in its mask.
///
/// Colourless generic pips take any mana, so only the coloured pips need a
/// matching; among matchings we maximise the chosen sources used on coloured
/// pips, and the leftover chosen sources must fit into the generic part.
pub fn payable(cost: &ManaCost, chosen: &[ManaMask], available: &[ManaMask]) -> bool {
    let need = cost.total() as usize;
    if chosen.len() > need || chosen.len() + available.len() < need {
        return false;
    }
    let mut groups: Vec<(ManaMask, bool, u8)> = Vec::with_capacity(8);
    for (&m, ch) in chosen.iter().map(|m| (m, true)).chain(available.iter().map(|m| (m, false))) {
        match groups.iter_mut().find(|g| g.0 == m && g.1 == ch) {
            Some(g) => g.2 += 1,
            None => groups.push((m, ch, 1)),
        }
    }
    let needs: Vec<(usize, u8)> = (0..6).filter(|&c| cost.pips[c] > 0).map(|c| (c, cost.pips[c])).collect();
    match best_chosen(&needs, &mut groups) {
        Some(used) => chosen.len() - used as usize <= cost.generic as usize,
        None => false,
    }
}

fn best_chosen(needs: &[(usize, u8)], groups: &mut [(ManaMask, bool, u8)]) -> Option<u8> {
    match needs.split_first() {
        None => Some(0),
        Some((&(c, k), rest)) => distribute(c, k, 0, rest, groups),
    }
}

fn distribute(c: usize, k: u8, gi: usize, rest: &[(usize, u8)], groups: &mut [(ManaMask, bool, u8)]) -> Option<u8> {
    if k == 0 {
        return best_chosen(rest, groups);
    }
    if gi == groups.len() {
        return None;
    }
    let (mask, ch, cnt) = groups[gi];
    let most = if mask & (1 << c) != 0 { k.min(cnt) } else { 0 };
    let mut best: Option<u8> = None;
    for t in (0..=most).rev() {
        groups[gi].2 -= t;
        if let Some(v) = distribute(c, k - t, gi + 1, rest, groups) {
            let v = v + if ch { t } else { 0 };
            best = Some(best.map_or(v, |b| b.max(v)));
        }
        groups[gi].2 += t;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const MU: ManaMask = 1 << U;
    const MW: ManaMask = 1 << W;
    const MR: ManaMask = 1 << R;

    #[test]
    fn parse_and_display() {
        let c = ManaCost::parse("{1}{U}{U}").unwrap();
        assert_eq!(c.generic, 1);
        assert_eq!(c.pips[U], 2);
        assert_eq!(c.to_string(), "{1}{U}{U}");
        assert_eq!(ManaCost::parse("{3}").unwrap().total(), 3);
        assert!(ManaCost::parse("{X}").is_none());
        assert!(ManaCost::parse("{W/U}").is_none());
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
    fn pool_pays_first() {
        let mut pool = ManaPool { amt: [0, 1, 2, 0, 0, 0] };
        let mut cost = ManaCost::parse("{1}{U}").unwrap();
        pool.pay_into(&mut cost);
        assert!(cost.is_zero());
        assert_eq!(pool.amt[B], 1);
    }
}
