//! Legacy Lands, lane C (ENGINE.md 270.3): the rules Choke, Torpor Orb,
//! Damping Sphere and The Tabernacle at Pendrell Vale need. (Pithing Needle
//! is Disruptor Flute's name choice and prohibition, `sneak_show.rs`; Cursed
//! Totem is Clarion Conqueror's `CardDef::locks_activated`.) Named after the
//! rules actions, each citing its rule; never after a card.
//!
//! * "<Permanents> don't untap during their controllers' untap steps" (CR
//!   502.3): [`LcDef::no_untap`], read by the untap step
//!   ([`Game::untap_prevented`]).
//! * "<Permanents> entering don't cause abilities to trigger" (CR 603.2,
//!   603.6a, 603.10): [`LcDef::entering_silent`]; an `Enters` event of such
//!   a permanent, as it is on the battlefield, triggers nothing
//!   ([`Game::entering_silenced`]).
//! * "If a land is tapped for two or more mana, it produces {C} instead of
//!   any other type and amount" (CR 106.12, 106.12b, 614.1a, 106.7):
//!   [`LcDef::damp_lands`], applied to every production of a land's mana
//!   ability with {T} in its cost ([`Game::damps`], `pay::productions`).
//! * "Each spell a player casts costs {n} more to cast for each other spell
//!   that player has cast this turn" (CR 601.2f): [`LcDef::tax_per_other_spell`]
//!   ([`Game::other_spells_tax`], `Game::cost_with`).
//! * "All creatures have '<triggered ability>'" (CR 113.7, 604.1, 613.1f):
//!   the granted ability is a trigger of the granting card's definition
//!   ([`LcDef::granted_to_creatures`]) that each creature on the battlefield
//!   has (unless it lost its abilities later, 613.7): each creature is its
//!   source, its controller the ability's controller (`Game::event`).
//!   "Destroy this creature unless you pay {1}" is `Op::OfferPay` then
//!   [`LC::DestroySource`].

use crate::db::Filter;
use crate::game::Game;
use crate::ids::CardIdx;
use crate::resolve::{Flow, Frame};

/// The printing facts of lane C's cards (`CardDef::lc`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct LcDef {
    /// "<Permanents matching it> don't untap during their controllers' untap
    /// steps" (CR 502.3; Choke: Islands).
    pub no_untap: Option<Filter>,
    /// "<Permanents matching it> entering don't cause abilities to trigger"
    /// (Torpor Orb: creatures).
    pub entering_silent: Option<Filter>,
    /// "If a land is tapped for two or more mana, it produces {C} instead of
    /// any other type and amount" (Damping Sphere).
    pub damp_lands: bool,
    /// "Each spell a player casts costs {n} more to cast for each other
    /// spell that player has cast this turn" (Damping Sphere): n.
    pub tax_per_other_spell: u8,
    /// The slots of the definition's triggered abilities that are not its
    /// own but every creature's: "All creatures have '<triggered ability>'"
    /// (The Tabernacle at Pendrell Vale).
    pub granted_to_creatures: Vec<u8>,
}

/// Which of lane C's rules some definition of the table has (`CardDb::lc`):
/// the hot paths look only then.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct LcDb {
    pub no_untap: bool,
    pub entering_silent: bool,
    pub damp_lands: bool,
    pub tax_per_other_spell: bool,
    pub granted_to_creatures: bool,
}

impl LcDb {
    pub(crate) fn of(defs: &[crate::db::CardDef]) -> LcDb {
        LcDb {
            no_untap: defs.iter().any(|d| d.lc.no_untap.is_some()),
            entering_silent: defs.iter().any(|d| d.lc.entering_silent.is_some()),
            damp_lands: defs.iter().any(|d| d.lc.damp_lands),
            tax_per_other_spell: defs.iter().any(|d| d.lc.tax_per_other_spell > 0),
            granted_to_creatures: defs.iter().any(|d| !d.lc.granted_to_creatures.is_empty()),
        }
    }
}

/// Lane C's instructions (`Ext::LandsC`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum LC {
    /// "Destroy this <permanent>": the ability's source, if it is still the
    /// same object on the battlefield (CR 400.7, 701.8a; indestructible and
    /// regeneration as for any destruction, 702.12b, 701.19a). A source
    /// that left does nothing.
    DestroySource,
}

impl LC {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            LC::DestroySource => false,
        }
    }
}

/// One {C} (the production Damping Sphere's replacement leaves).
const ONE_C: [u8; 6] = [0, 0, 0, 0, 0, 1];

impl Game {
    pub(crate) fn exec_lands_c(&mut self, f: &mut Frame, op: &LC) -> Flow {
        match op {
            LC::DestroySource => {
                if let Some(&c) = self.objs_on(f, &crate::program::On::Source).first() {
                    self.note(|g| format!("{} is destroyed", g.name(c)));
                    self.destroy(c);
                }
                Flow::Next
            }
        }
    }

    /// Does a permanent's static ability keep permanent `c` from untapping
    /// during its controller's untap step (CR 502.3: "<permanents> don't
    /// untap during their controllers' untap steps"; the permanent as it is
    /// then, its subtypes after the layers, 613)?
    pub(crate) fn untap_prevented(&self, c: CardIdx) -> bool {
        if !self.db.lc.no_untap {
            return false;
        }
        self.battlefield.iter().any(|&s| match &self.abilities(s).lc.no_untap {
            Some(f) => {
                let ctx = crate::cast::Ctx { controller: self.objs[s.i()].controller, source: Some(self.obj_ref(s)) };
                self.card_matches(f, c, &ctx)
            }
            None => false,
        })
    }

    /// Is `ev` an `Enters` event of a permanent that a static ability says
    /// causes no triggered ability to trigger ("creatures entering don't
    /// cause abilities to trigger": the permanent as it exists on the
    /// battlefield, with the continuous effects, the rulings; a permanent
    /// with the ability entering at the same time counts, an entry's events
    /// being matched once all of it is on the battlefield,
    /// `Game::enters_together`)?
    pub(crate) fn entering_silenced(&self, ev: &crate::triggers::Event) -> bool {
        if !self.db.lc.entering_silent || ev.kind != crate::db::EventKind::Enters {
            return false;
        }
        let crate::triggers::Subj::Obj(r) = ev.a else { return false };
        if !self.live(r) || self.objs[r.idx.i()].zone != crate::ids::Zone::Battlefield {
            return false;
        }
        self.battlefield.iter().any(|&s| match &self.abilities(s).lc.entering_silent {
            Some(f) => {
                let ctx = crate::cast::Ctx { controller: self.objs[s.i()].controller, source: Some(self.obj_ref(s)) };
                self.card_matches(f, r.idx, &ctx)
            }
            None => false,
        })
    }

    /// Would mana ability productions of `src` (`tap`: its cost has {T}) be
    /// replaced by "if a land is tapped for two or more mana, it produces {C}
    /// instead" (CR 106.12b: a land, tapped for mana, by a mana ability with
    /// {T}; a permanent on the battlefield with the static ability)?
    pub(crate) fn damps(&self, src: Option<CardIdx>, tap: bool) -> bool {
        tap && self.db.lc.damp_lands
            && src.is_some_and(|c| self.is_type(c, crate::db::types::LAND))
            && self.battlefield.iter().any(|&s| self.abilities(s).lc.damp_lands)
    }

    /// The productions `prods` of a land tapped for mana with the
    /// replacement applied (`damps`): each of two or more mana becomes one
    /// {C} (the amount is the ability's own: a triggered mana ability's
    /// "additional" mana is its own production, CR 605.1b, 106.12a). Two such
    /// effects: after one applies the production is one mana and the other
    /// no longer applies (CR 616.1f). In order, without repeats.
    pub(crate) fn damped(prods: Vec<[u8; 6]>) -> Vec<[u8; 6]> {
        let mut out: Vec<[u8; 6]> = Vec::with_capacity(prods.len());
        for v in prods {
            let total: u32 = v.iter().map(|&x| x as u32).sum();
            let w = if total >= 2 { ONE_C } else { v };
            if !out.contains(&w) {
                out.push(w);
            }
        }
        out
    }

    /// The increase to the cost of a spell `p` casts by "each spell a player
    /// casts costs {n} more to cast for each other spell that player has cast
    /// this turn" (CR 601.2f; the spells `p` cast this turn before it, whether
    /// or not the permanent was there then: its ruling), each such
    /// permanent's.
    pub(crate) fn other_spells_tax(&self, p: crate::ids::PlayerId) -> u64 {
        if !self.db.lc.tax_per_other_spell {
            return 0;
        }
        let others = self.players[p as usize].spells_cast as u64;
        if others == 0 {
            return 0;
        }
        self.battlefield.iter().map(|&s| self.abilities(s).lc.tax_per_other_spell as u64).sum::<u64>().saturating_mul(others)
    }
}
