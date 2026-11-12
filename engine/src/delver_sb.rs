//! The Legacy Izzet Delver 75's sideboard (ENGINE.md 122): the one rules
//! action its five cards needed that no op expressed. Named after the rules
//! action, citing its rule; never after a card.
//!
//! * "<This> deals damage to each player equal to [twice] the number of
//!   <permanents> that player controls" (CR 120.3, 120.8, 608.2h):
//!   [`DelverSb::DamageEachPlayerByCount`].
//!
//! The other parts of the five lower onto existing ops: Consign to Memory's
//! "counter target triggered ability or colorless spell" is `Op::Counter`
//! over a stack-object requirement (ENGINE.md 27.2) and replicate the
//! existing additional cost and cast trigger (26.5); Hydroblast is
//! Pyroblast's `ModalChooseOne` with its colour checked on resolution;
//! Meltdown is `Op::DestroyAll` with `Filter::MvAtMostX`; Rough // Tumble is
//! a split card (ENGINE.md 25, CR 709) whose halves are `Op::DamageAll`.

use crate::cast::Ctx;
use crate::db::Filter;
use crate::game::Game;
use crate::ids::{opp, Target};
use crate::resolve::{Flow, Frame};

/// The Delver sideboard's instructions (`Ext::DelverSb`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum DelverSb {
    /// The source deals damage to each player equal to `times` times the
    /// number of permanents matching `filter` that player controls (CR
    /// 120.3: one damage event per player, its results by the source's
    /// characteristics; 120.8: none to a player whose number is 0). Every
    /// player's number is counted before any damage is dealt, once, as the
    /// effect is applied (608.2h); the damage is dealt in APNAP order (no
    /// state-based action is checked in between, 704.3). `filter` names no
    /// controller (the lowering refuses one): "that player controls" is the
    /// grouping.
    DamageEachPlayerByCount { filter: Filter, times: u8 },
}

impl DelverSb {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            DelverSb::DamageEachPlayerByCount { .. } => false,
        }
    }
}

impl Game {
    pub(crate) fn exec_delver_sb(&mut self, f: &mut Frame, op: &DelverSb) -> Flow {
        match op {
            DelverSb::DamageEachPlayerByCount { filter, times } => {
                let src = self.frame_damage_source(f);
                let ctx = Ctx { controller: f.controller, source: Some(f.source) };
                let ap = self.turn.active;
                let amounts: Vec<(u8, crate::num::Wide)> = [ap, opp(ap)]
                    .into_iter()
                    .map(|p| {
                        let n = self.permanents_of(p).filter(|&c| self.card_matches(filter, c, &ctx)).count();
                        (p, n as crate::num::Wide * *times as crate::num::Wide)
                    })
                    .collect();
                for (p, n) in amounts {
                    // (0: no damage at all, CR 120.8; `deal_damage` deals none.)
                    self.note(|_| format!("P{p}: {n} damage (each player, by count)"));
                    self.deal_damage(src, Target::Player(p), n);
                }
                Flow::Next
            }
        }
    }
}
