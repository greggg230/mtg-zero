//! Lane A of the Edge of Eternities port (ENGINE.md 99.2): station (CR
//! 702.184, 722), counters, artifacts, Equipment and Auras, as the engine
//! resolves them.

use crate::game::Game;
use crate::ids::Zone;
use crate::resolve::Frame;

impl Game {
    /// `Amount::TappedPower`: the power of the creature tapped to pay the
    /// resolving ability's cost (its first noted object, `StackItem::noted`),
    /// now if it is still on the battlefield, else as it last existed there
    /// (CR 702.184a, 608.2h; station's ruling: "use the tapped creature's
    /// power as the station ability resolves"). None noted: 0.
    pub(crate) fn tapped_power(&self, f: &Frame) -> i32 {
        let Some(&r) = f.noted.first() else { return 0 };
        if self.live(r) && self.objs[r.idx.i()].zone == Zone::Battlefield {
            return self.power(r.idx);
        }
        match self.objs[r.idx.i()].lki_power {
            Some((inc, p)) if inc == r.inc => p,
            _ => 0,
        }
    }
}
