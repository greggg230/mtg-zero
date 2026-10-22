//! The effect instruction set (ENGINE.md 7). An ability's program is a flat
//! `Vec<Op>`; control flow is forward jumps. Ops name rules actions, never
//! cards; new IR classes should lower onto existing ops where the CR
//! semantics match.

use crate::db::{AbilityDef, Filter, TargetReq};
use crate::mana::ManaCost;

/// Which player an op acts on.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Who {
    /// The ability's controller.
    You,
    /// The player chosen for target requirement `i`.
    TargetPlayer(u8),
    /// The controller of the object chosen for target requirement `i`.
    ControllerOfTarget(u8),
}

/// A number an op uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Amount {
    Fixed(i32),
}

/// Where a countered spell goes (CR 701.5a; Memory Lapse).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpellDest {
    Graveyard,
    LibraryTop,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Op {
    /// The source deals `amount` damage to each legal target of requirement
    /// `to` (CR 120).
    DealDamage { amount: Amount, to: u8 },
    /// `who` draws `n` cards, one at a time (CR 121.2).
    Draw { who: Who, n: Amount },
    /// `who` gains `n` life (CR 119).
    GainLife { who: Who, n: Amount },
    /// Add mana to `who`'s pool (CR 106.4), counts per type WUBRGC.
    AddMana { who: Who, mana: [u8; 6] },
    /// Counter each legal target spell of requirement `tgt` (CR 701.5).
    Counter { tgt: u8, dest: SpellDest },
    /// `who` may pay `cost`; if they do, jump to `if_paid` (CR 118.12,
    /// "... unless its controller pays"). Choice: yes/no, then `PayMana`.
    OfferPay { who: Who, cost: ManaCost, if_paid: u16 },
    /// `who` puts `n` cards from their hand on top of their library, one
    /// decision per card; the last chosen ends on top (Brainstorm).
    PutFromHandOnTop { who: Who, n: u8 },
    /// `who` discards `n` cards at random (CR 701.9b). Chance keys.
    DiscardRandom { who: Who, n: Amount },
    /// Destroy each legal target of requirement `tgt` (CR 701.7).
    Destroy { tgt: u8 },
    /// Exile each legal target of requirement `tgt` (CR 406.2).
    Exile { tgt: u8 },
    /// `who` takes an extra turn after this one (CR 500.7).
    ExtraTurn { who: Who },
}

/// Builds an [`AbilityDef`] during lowering.
#[derive(Default)]
pub struct Builder {
    pub targets: Vec<TargetReq>,
    pub program: Vec<Op>,
}

impl Builder {
    /// Add a target requirement; returns its index.
    pub fn target(&mut self, filter: Filter, min: u8, max: u8) -> u8 {
        self.targets.push(TargetReq { filter, min, max });
        (self.targets.len() - 1) as u8
    }

    pub fn op(&mut self, op: Op) -> usize {
        self.program.push(op);
        self.program.len() - 1
    }

    /// Current end of the program (a jump target).
    pub fn here(&self) -> u16 {
        self.program.len() as u16
    }

    pub fn finish(self) -> AbilityDef {
        AbilityDef { targets: self.targets, program: self.program }
    }
}
