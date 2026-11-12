//! Lane C of the Legacy Doomsday cards (ENGINE.md 124.3): landwalk (Street
//! Wraith's swampwalk, CR 702.14) and "if a card would be put into an
//! opponent's graveyard from anywhere, exile it instead" (Leyline of the
//! Void, CR 614.1a). Cycling with a non-mana cost (Street Wraith's "Pay 2
//! life", Edge of Autumn's "Sacrifice a land", CR 702.29a) needs nothing
//! here: its costs are the activated-ability costs every ability pays.

use crate::db::{types, Kw, KwSet};
use crate::game::Game;
use crate::ids::{opp, CardIdx, PlayerId};

/// The landwalk keywords and the basic land type each walks (CR 702.14a,
/// 305.6).
pub const LANDWALKS: [(Kw, &str); 5] = [
    (Kw::Plainswalk, "Plains"),
    (Kw::Islandwalk, "Island"),
    (Kw::Swampwalk, "Swamp"),
    (Kw::Mountainwalk, "Mountain"),
    (Kw::Forestwalk, "Forest"),
];

/// The landwalk keywords' bits in a [`KwSet`].
const LANDWALK_BITS: u32 = (1 << Kw::Plainswalk as u32)
    | (1 << Kw::Islandwalk as u32)
    | (1 << Kw::Swampwalk as u32)
    | (1 << Kw::Mountainwalk as u32)
    | (1 << Kw::Forestwalk as u32);

impl Game {
    /// CR 702.14c, 509.1b: an attacker with keywords `ka` has a landwalk the player who would
    /// block it with `b` (the defending player: `b`'s controller) makes it
    /// unblockable by: that player controls at least one land with the
    /// walked land type (its land types as they are now: layer 4, so a
    /// nonbasic land under a "nonbasic lands are Mountains" effect has only
    /// Mountain, CR 305.7). Landwalk abilities don't cancel one another
    /// (702.14d): `b`'s own keywords don't matter.
    pub(crate) fn landwalk_unblockable(&self, ka: KwSet, b: CardIdx) -> bool {
        if ka.0 & LANDWALK_BITS == 0 {
            return false;
        }
        let dp = self.objs[b.i()].controller;
        LANDWALKS.iter().any(|&(k, name)| {
            ka.has(k)
                && self.db.subtype_id(name).is_some_and(|s| {
                    self.permanents_of(dp).any(|x| self.types_of(x) & types::LAND != 0 && self.has_subtype(x, s))
                })
        })
    }

    /// Does "if a card would be put into an opponent's graveyard from
    /// anywhere, exile it instead" (Leyline of the Void, CR 614.1a) apply to
    /// a card `owner` owns? A permanent an opponent of `owner` controls has
    /// the ability now (with its abilities as they are: one that lost all
    /// abilities has none, 613.1f), or had it as it left the battlefield
    /// earlier in the simultaneous move in progress (the replacement
    /// exists as the event begins: `Lki::opp_gy_exile`, recorded before it
    /// moved).
    pub(crate) fn opp_gy_exile_for(&self, owner: PlayerId) -> bool {
        let q = opp(owner);
        self.permanents_of(q).any(|c| self.abilities(c).opp_gy_exile)
            || self.batching && self.leaving.iter().any(|l| l.controller == q && l.opp_gy_exile)
    }
}
