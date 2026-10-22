//! The effect instruction set (ENGINE.md 7). An ability's program is a flat
//! `Vec<Op>`; control flow is forward jumps. Ops name rules actions, never
//! cards; new IR classes should lower onto existing ops where the CR
//! semantics match.

use crate::db::{AbilityDef, Cond, Filter, ManaProd, TargetReq};
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
    /// The controller of the object last remembered (`Op::Remember`, last
    /// known information).
    LkiController,
    /// The active player / the other player (2p): "each player" effects run
    /// in APNAP order (CR 101.4).
    Active,
    NonActive,
    /// The controller's opponent ("each opponent", 2p).
    Opponent,
}

/// A number an op uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Amount {
    Fixed(i32),
    /// The power of the object last remembered (`Op::Remember`).
    LkiPower,
}

/// Where a countered spell goes (CR 701.5a; Memory Lapse).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpellDest {
    Graveyard,
    LibraryTop,
    Exile,
    Hand,
}

/// Where a searched-for card goes (CR 701.19).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SearchDest {
    Hand,
    /// "Then shuffle and put that card on top."
    Top,
    Graveyard,
    Battlefield { tapped: bool },
}

/// Where cards of a looked-at pile go (scry, surveil, "look at the top N").
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PileDest {
    /// On top of the library, in the order chosen (the last on top).
    Top,
    /// On the bottom, in the order chosen (the last at the very bottom).
    Bottom,
    Graveyard,
    Hand,
}

/// `PileTo.max`: any number of the cards.
pub const PILE_ANY: u8 = u8::MAX;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Op {
    /// The source deals `amount` damage to each legal target of requirement
    /// `to` (CR 120).
    DealDamage { amount: Amount, to: u8 },
    /// The source deals `amount` damage to each creature matching `filter`
    /// (CR 120; all at once).
    DamageAll { amount: Amount, filter: Filter },
    /// The source deals `amount` damage to player `who`.
    DamagePlayer { amount: Amount, who: Who },
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
    /// Return each legal target of requirement `tgt` (a permanent or a
    /// spell) to its owner's hand.
    ReturnToHand { tgt: u8 },
    /// Untap each legal target of requirement `tgt`.
    Untap { tgt: u8 },
    /// `who` discards `n` cards of their choice, one decision per card
    /// (CR 701.9a).
    Discard { who: Who, n: u8 },
    /// `who` discards their hand.
    DiscardHand { who: Who },
    /// `who` loses `n` life (CR 119.3).
    LoseLife { who: Who, n: Amount },
    /// Remember the first legal target of requirement `tgt` as it is now
    /// (power, controller): last known information for later ops after it
    /// leaves (CR 608.2h).
    Remember { tgt: u8 },
    /// Destroy every permanent matching `filter` at once (CR 701.7).
    DestroyAll { filter: Filter },
    /// Put each legal target of requirement `tgt` into its owner's library
    /// with `depth` cards above it (0 = on top).
    PutInLibrary { tgt: u8, depth: u8 },
    /// `who` adds one of the productions of `prod` (CR 106.1b: they choose).
    AddManaChoice { who: Who, prod: ManaProd },
    /// Continue at `to` (the end of a "then" branch).
    Jump { to: u16 },
    /// Unless `cond` holds for the controller now, continue at `to`
    /// (conditions are checked as the effect resolves).
    JumpUnless { cond: Cond, to: u16 },
    /// Unless the legal targets of requirement `tgt` all match `filter` now,
    /// continue at `to` ("destroy target creature if it has mana value 2 or
    /// less").
    JumpUnlessTarget { tgt: u8, filter: Filter, to: u16 },
    /// `who` may do what follows ("you may"): no continues at `skip`.
    /// Choice: yes/no.
    May { who: Who, skip: u16 },
    /// `who` searches their library for a card matching `filter`, puts it
    /// into `dest` (revealing it first if `reveal`), then shuffles (CR
    /// 701.19). With `may_fail` (a search for a card with a stated
    /// quality, 701.19b) they may find nothing; otherwise they must find a
    /// card if the library has one (701.19c). Choice: a definition.
    Search { who: Who, filter: Filter, dest: SearchDest, reveal: bool, may_fail: bool },
    /// `who` looks at the top `n` cards of their library (all of them if
    /// fewer): they become the frame's pile, known to `who` (CR 701.18a,
    /// 701.42a); with `reveal`, revealed to everyone. Keys per card.
    LookTop { who: Who, n: u8, reveal: bool },
    /// `who` puts cards of the pile into `dest`, one decision per card:
    /// with `rest`, every card left (only their order is chosen);
    /// otherwise between `min` and `max` (`PILE_ANY`) of those matching
    /// `filter` (as many as there are, if fewer). Top and bottom take the
    /// cards in the order chosen; a hand or graveyard takes a set, chosen in
    /// canonical order. With `reveal`, each is revealed as it moves.
    PileTo { who: Who, dest: PileDest, filter: Filter, min: u8, max: u8, rest: bool, reveal: bool },
    /// `who` shuffles their library (CR 701.20).
    Shuffle { who: Who },
    /// The ability's source, if it is still the same permanent, goes on top
    /// of its owner's library.
    SourceToLibraryTop,
    /// The controller creates `n` tokens of token definition `token` (CR
    /// 111.2, 701.6), tapped if `tapped`.
    CreateToken { token: u16, n: u8, tapped: bool },
    /// `who` sacrifices a permanent matching `filter` of their choice, if
    /// they control one (CR 701.17). Choice: one of their permanents.
    Sacrifice { who: Who, filter: Filter },
    /// `whose` hand is revealed to everyone (`to: None`, CR 701.16) or
    /// looked at by `to`. Keys per card, in definition order.
    RevealHand { whose: Who, to: Option<Who> },
    /// `chooser` chooses a card matching `filter` from `whose`'s hand, which
    /// they have seen; that player discards it ("you choose a ... card from
    /// it; that player discards that card"). With `optional` the chooser
    /// may choose nothing.
    ChooseDiscard { chooser: Who, whose: Who, filter: Filter, optional: bool },
}

impl Op {
    /// The forward jump target of a control-flow op.
    fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            Op::Jump { to } | Op::JumpUnless { to, .. } | Op::JumpUnlessTarget { to, .. } => Some(to),
            Op::May { skip, .. } => Some(skip),
            Op::OfferPay { if_paid, .. } => Some(if_paid),
            _ => None,
        }
    }
}

/// Builds an [`AbilityDef`] during lowering.
#[derive(Default)]
pub struct Builder {
    pub targets: Vec<TargetReq>,
    pub program: Vec<Op>,
    /// While lowering the second branch of a conditional, its target
    /// requirements must repeat the first branch's from this index (one
    /// target described in both branches); `reuse_failed` records a
    /// mismatch.
    pub reuse: Option<usize>,
    pub reuse_failed: bool,
}

impl Builder {
    /// Add a target requirement; returns its index.
    pub fn target(&mut self, filter: Filter, min: u8, max: u8) -> u8 {
        let req = TargetReq { filter, min, max };
        if let Some(k) = self.reuse {
            if self.targets.get(k) == Some(&req) {
                self.reuse = Some(k + 1);
                return k as u8;
            }
            self.reuse_failed = true;
        }
        self.targets.push(req);
        (self.targets.len() - 1) as u8
    }

    /// Point the jump of the op at `at` to the current end.
    pub fn patch(&mut self, at: usize) {
        let here = self.here();
        *self.program[at].jump_target().expect("a jump op") = here;
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
        AbilityDef { targets: self.targets, program: self.program, modes: None }
    }
}
