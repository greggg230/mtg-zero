//! Unbounded numbers (ENGINE.md 46). Magic has no maximum for counters,
//! life, power, toughness or damage (CR 107.1, 122.1, 119, 120.3, 208), and
//! an exponential board (Ouroboroid, Bristly Bill, Scythecat Cub) reaches
//! any number in a 60-turn game. The engine keeps every such value exact up
//! to its type's end and **saturates** there, never wrapping or truncating
//! (a wrong number that looks fine is the worst outcome: DESIGN.md 3.1). The
//! bounds are class-B engineering limits (docs/shortcuts.md 46.1-46.4):
//!
//! * a count of counters of one kind on one object or player (+1/+1,
//!   -1/-1, loyalty, lore, charge, poison, energy, experience, ...): [`Count`]
//!   (`u32`), exact up to 4,294,967,295;
//! * power and toughness (layer 7, computed in `i64`), an amount of damage,
//!   life, a life total, an effect's amount: `i32`, exact within
//!   +-2,147,483,647 ([`AMOUNT_MAX`]; symmetric, so negating never
//!   overflows);
//! * marked damage and an event's amount: `u32` (at least any amount);
//! * mana of one type in a pool: `u8` (255), the payment's arithmetic.
//!
//! Every conversion between them goes through these functions (saturating,
//! with the reason at the call site), never a bare `as` that could
//! truncate.

/// A count of counters of one kind (CR 122.1): exact up to `u32::MAX`, then
/// saturating (shortcuts.md 46.1).
pub type Count = u32;

/// The largest magnitude of an amount (power, toughness, damage, life, an
/// effect's number): `i32::MAX`. Amounts saturate within
/// `-AMOUNT_MAX..=AMOUNT_MAX` (shortcuts.md 46.2).
pub const AMOUNT_MAX: i32 = i32::MAX;

/// An amount (power, damage, life, an effect's X) as a number of counters
/// to put or remove: a negative one is 0 (CR 107.1b), a larger one than a
/// count holds saturates.
#[inline]
pub fn count(n: i64) -> Count {
    n.clamp(0, Count::MAX as i64) as Count
}

/// A wide intermediate (sums of power, counters added to printed values)
/// as an amount, saturating at +-[`AMOUNT_MAX`].
#[inline]
pub fn amount(n: i64) -> i32 {
    n.clamp(-(AMOUNT_MAX as i64), AMOUNT_MAX as i64) as i32
}

/// A count of counters read as an amount ("X is the number of counters on
/// it"), saturating at [`AMOUNT_MAX`].
#[inline]
pub fn count_amount(n: Count) -> i32 {
    n.min(AMOUNT_MAX as Count) as i32
}

/// A non-negative amount (damage dealt, an event's amount) as an unsigned
/// quantity (marked damage, `Event::amount`): a negative one is 0.
#[inline]
pub fn unsigned(n: i32) -> u32 {
    n.max(0) as u32
}

/// An unsigned quantity (marked damage, an event's amount) as an amount,
/// saturating at [`AMOUNT_MAX`].
#[inline]
pub fn signed(n: u32) -> i32 {
    n.min(AMOUNT_MAX as u32) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_saturate() {
        assert_eq!(count(-5), 0);
        assert_eq!(count(70_000), 70_000);
        assert_eq!(count(i64::MAX), u32::MAX);
        assert_eq!(amount(i64::MAX), i32::MAX);
        assert_eq!(amount(i64::MIN), -i32::MAX);
        assert_eq!(amount(-70_000), -70_000);
        assert_eq!(count_amount(u32::MAX), i32::MAX);
        assert_eq!(unsigned(-1), 0);
        assert_eq!(signed(u32::MAX), i32::MAX);
    }
}
