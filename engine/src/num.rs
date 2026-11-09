//! Unbounded numbers (ENGINE.md 46, 100.7). Magic has no maximum for
//! counters, life, power, toughness or damage (CR 107.1, 122.1, 119, 120.3,
//! 208), and an exponential board (Ouroboroid, Bristly Bill, Scythecat Cub)
//! reaches any number in a 60-turn game. Every number the game keeps is
//! **exact**, or the game ends **aborted** (`EndReason::Aborted`, `Game::abort`:
//! no result, self-play drops it). Nothing saturates, wraps or truncates (a
//! wrong number that looks fine is the worst outcome: DESIGN.md 3.1).
//!
//! * **Stored** quantities are 64-bit: a count of counters of one kind on an
//!   object or player (+1/+1, -1/-1, loyalty, lore, charge, poison, energy,
//!   experience, ...) is a [`Count`] (`u64`) within `0..=`[`COUNT_MAX`]
//!   (`i64::MAX`, so a count always reads exactly as an amount); an amount
//!   (a life total, life gained this turn, marked damage, an event's amount,
//!   damage dealt, the numbers continuous effects and last known information
//!   hold) is an `i64` within `+-`[`AMOUNT_MAX`] (symmetric: negating never
//!   overflows; marked damage and event amounts are `u64` within it).
//! * **Derived** values are [`Wide`] (`i128`): power and toughness (layer 7),
//!   `Game::amount`, every sum or product of amounts (total power, crew,
//!   "twice X", P + T, half a life total). A sum of `i64` terms can't
//!   overflow an `i128` in any game that fits in memory (2^63 terms of 2^63),
//!   so comparisons in `Wide` are exact.
//! * A derived value goes into a stored field through [`Game::exact`] or
//!   [`Game::exact_count`]: out of range, the game ends aborted (and the
//!   value is clamped: the running task finishes with it, nothing reads it
//!   after). Code that can't abort (`&self`) computes in `Wide` and returns
//!   `Wide`.
//!
//! Rules floors are not saturation and stay where the rules put them: "put X
//! counters" with X < 0 puts none (CR 107.1b), "remove n counters" removes
//! at most those there (122.8-like), damage of 0 or less isn't dealt (120.8),
//! a cost reduction doesn't take generic mana below 0 (601.2f).
//!
//! Mana of one type in a pool is a `u8` (the payment model, workstream B):
//! past 255 the game aborts (`ManaPool::add` reports it).

use crate::game::Game;

/// A count of counters of one kind (CR 122.1): within `0..=COUNT_MAX`.
pub type Count = u64;

/// The largest count: `i64::MAX`, so a count reads exactly as an amount.
pub const COUNT_MAX: Count = i64::MAX as u64;

/// The largest magnitude of a stored amount (life, damage, an effect's
/// number): `i64::MAX`, symmetric (`-AMOUNT_MAX..=AMOUNT_MAX`).
pub const AMOUNT_MAX: i64 = i64::MAX;

/// A derived value (power, toughness, `Game::amount`, sums and products of
/// amounts): exact for every game that fits in memory.
pub type Wide = i128;

/// A count read as an amount ("X is the number of counters on it"): exact,
/// a count being at most `COUNT_MAX` = `AMOUNT_MAX`.
#[inline]
pub fn count_amount(n: Count) -> i64 {
    debug_assert!(n <= COUNT_MAX, "count {n} past COUNT_MAX");
    n as i64
}

/// A derived number of tokens to create (`Game::create_tokens`'s `u32`):
/// a negative number is none (CR 107.1b), and any number past `u32::MAX`
/// creates exactly what `u32::MAX` does: `Game::token_count` creates at most
/// the 96 permanents a player may have (`act::PERM_N`, docs/shortcuts.md
/// 26.11.6), so the result is exact.
#[inline]
pub fn tokens_n(v: Wide) -> u32 {
    v.clamp(0, u32::MAX as Wide) as u32
}

/// A derived number of cards to move from the top of a zone (mill, look at
/// the top n): a negative number is none (CR 107.1b), and past `usize::MAX`
/// (only on a 32-bit target) it is every card, as `usize::MAX` is: exact.
#[inline]
pub fn cards_n(v: Wide) -> usize {
    v.clamp(0, usize::MAX as Wide) as usize
}

/// Is `v` a storable amount (`|v| <= AMOUNT_MAX`)?
#[inline]
pub fn amount_fits(v: Wide) -> bool {
    (-(AMOUNT_MAX as Wide)..=AMOUNT_MAX as Wide).contains(&v)
}

/// Is `v` a storable count (`0..=COUNT_MAX`)?
#[inline]
pub fn count_fits(v: Wide) -> bool {
    (0..=COUNT_MAX as Wide).contains(&v)
}

impl Game {
    /// A derived value stored as an amount (a life total, marked damage, an
    /// effect's number): exact within +-`AMOUNT_MAX`; past it the game ends
    /// aborted (`what` names the number) and the result is clamped.
    pub(crate) fn exact(&mut self, v: Wide, what: &str) -> i64 {
        if amount_fits(v) {
            return v as i64;
        }
        self.abort(|| format!("{what} {v} is past the exact range (+-{AMOUNT_MAX})"));
        v.clamp(-(AMOUNT_MAX as Wide), AMOUNT_MAX as Wide) as i64
    }

    /// A derived value stored as a count (counters, poison) or a
    /// non-negative amount (marked damage, an event's amount): exact within
    /// `0..=COUNT_MAX`; outside it the game ends aborted and the result is
    /// clamped. (A negative number of counters to put is a rules floor, CR
    /// 107.1b: callers apply it first.)
    pub(crate) fn exact_count(&mut self, v: Wide, what: &str) -> Count {
        if count_fits(v) {
            return v as Count;
        }
        self.abort(|| format!("{what} {v} is past the exact range (0..={COUNT_MAX})"));
        v.clamp(0, COUNT_MAX as Wide) as Count
    }

    /// A derived value used as a `u32` count of actions (cards drawn or
    /// milled, "do this n times"): exact within `0..=u32::MAX`; past it the
    /// game ends aborted. (A negative number is a rules floor, CR 107.1b:
    /// callers apply it first.)
    pub(crate) fn exact_u32(&mut self, v: Wide, what: &str) -> u32 {
        match u32::try_from(v) {
            Ok(n) => n,
            Err(_) => {
                self.abort(|| format!("{what} {v} is past the exact range (0..={})", u32::MAX));
                v.clamp(0, u32::MAX as Wide) as u32
            }
        }
    }

    /// A derived value as mana of one type (the pool's and the payment's
    /// `u8`, workstream B's model): exact within `0..=255`; past it the game
    /// ends aborted.
    pub(crate) fn exact_u8(&mut self, v: Wide, what: &str) -> u8 {
        match u8::try_from(v) {
            Ok(n) => n,
            Err(_) => {
                self.abort(|| format!("{what} {v} is past the exact range (0..=255)"));
                v.clamp(0, u8::MAX as Wide) as u8
            }
        }
    }

    /// A `u32` bookkeeping count (spells cast, cards drawn, an object's
    /// incarnation, a timestamp) plus one: exact, or the game ends aborted
    /// at `u32::MAX` (it stays there).
    pub(crate) fn exact_inc(&mut self, v: u32, what: &str) -> u32 {
        match v.checked_add(1) {
            Some(n) => n,
            None => {
                self.abort(|| format!("{what} is past {}", u32::MAX));
                v
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ranges_are_symmetric_and_a_count_reads_as_an_amount() {
        assert_eq!(COUNT_MAX as i64, AMOUNT_MAX);
        assert_eq!(count_amount(COUNT_MAX), i64::MAX);
        assert!(amount_fits(-(i64::MAX as Wide)) && amount_fits(i64::MAX as Wide));
        assert!(!amount_fits(i64::MIN as Wide) && !amount_fits(i64::MAX as Wide + 1));
        assert!(count_fits(0) && count_fits(COUNT_MAX as Wide));
        assert!(!count_fits(-1) && !count_fits(COUNT_MAX as Wide + 1));
    }
}

