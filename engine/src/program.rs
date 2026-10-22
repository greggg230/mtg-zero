//! The effect instruction set (ENGINE.md 7). An ability's program is a flat
//! `Vec<Op>`; control flow is forward jumps. Ops name rules actions, never
//! cards; new IR classes should lower onto existing ops where the CR
//! semantics match.

use crate::db::{AbilityDef, Cond, Counter, Filter, ManaProd, Mod, TargetReq};
use crate::layers::Until;
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
    /// The player of the triggered ability's event ("that player").
    EventPlayer,
}

/// A number an op uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Amount {
    Fixed(i32),
    /// The power of the object last remembered (`Op::Remember`).
    LkiPower,
    /// The amount of the triggered ability's event ("that much damage").
    EventAmount,
    /// The source's power now (its last known power if it left).
    SourcePower,
    /// The mana value of the card last remembered (`Ext::RevealTopToHand`).
    LkiManaValue,
}

/// Which objects an `Ext` op acts on.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum On {
    /// Each legal target of requirement i.
    Target(u8),
    /// The ability's source, if it is still the same object on the
    /// battlefield.
    Source,
    /// The object of the triggered ability's event ("that creature"), if
    /// it is still the same object on the battlefield.
    EventObj,
    /// Each permanent matching the filter at resolution ("creatures you
    /// control"; "you" is the controller, "another" excludes the source).
    Each(Filter),
}

/// Effect instructions added after the network layout v1 fixed the list of
/// ops (`az::encode` encodes them as one kind). Same rules as `Op`: named
/// after rules actions, never cards.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Ext {
    /// Each object of `on` gets the mods (CR 611.2; the objects are locked
    /// in at resolution, 611.2c), until `until`.
    Apply { on: On, mods: Vec<Mod>, until: Until },
    /// A rules effect (no characteristic changes) on every permanent
    /// matching the filter at any time while it lasts ("creatures target
    /// player controls can't block this turn"); the filter's "you" is
    /// player `who`.
    ApplyRules { filter: Filter, who: Who, m: Mod, until: Until },
    /// Each object of `on` gains protection from the colour of the
    /// controller's choice (or colorless if `colorless`) until `until`.
    /// Choice: `NUMBER` = colour index W U B R G, 5 = colorless.
    ProtectionChoice { on: On, colorless: bool, until: Until },
    /// Put n counters of a kind on each object of `on` (CR 122.6).
    PutCounters { on: On, counter: Counter, n: Amount },
    /// Tap each object of `on`.
    Tap { on: On },
    /// Untap each object of `on` (a stun counter is removed instead).
    Untap { on: On },
    /// Attach the source (an Equipment) to the first legal target of
    /// requirement `tgt` (CR 701.3; equip, 702.6).
    Attach { tgt: u8 },
    /// The source fights the first legal target of requirement `tgt` (CR
    /// 701.14): each deals damage equal to its power to the other.
    Fight { tgt: u8 },
    /// `who` mills n cards (CR 701.13).
    Mill { who: Who, n: Amount },
    /// `who` reveals the top card of their library and puts it into their
    /// hand; its mana value is remembered (`Amount::LkiManaValue`).
    RevealTopToHand { who: Who },
    /// Unless the source is still on the battlefield and matches `filter`,
    /// continue at `to` ("if this creature is a Spirit").
    JumpUnlessSource { filter: Filter, to: u16 },
    /// "[You may] discard a card. If you do, ...": the controller discards
    /// a card of their choice (or, if `optional`, declines), else continues
    /// at `skip` (also with an empty hand). Choice: a hand card, or `PASS`.
    DiscardThen { skip: u16, optional: bool },
    /// `who` chooses between `min` and `max` permanents matching `filter`
    /// (not targeted; "you" in the filter is the ability's controller), one
    /// decision per permanent in increasing slot order (`PASS` = no more),
    /// then `act` happens to all of them at once.
    Choose { who: Who, filter: Filter, min: u8, max: u8, act: ChooseAct },
    /// Exile each legal target of requirement `tgt`, then return those cards
    /// to the battlefield under their owners' control (a token ceases to
    /// exist instead).
    Flicker { tgt: u8 },
    /// Double the counters of a kind on each object of `on` (CR 701.10e-like:
    /// put that many more).
    DoubleCounters { on: On, counter: Counter },
    /// The controller flips a coin (CR 705): winning continues at `won`,
    /// losing at the next op. A public chance key.
    FlipCoin { won: u16 },
    /// Unless the object last remembered (`Op::Remember`) was controlled by
    /// the ability's controller, continue at `to` ("if you controlled that
    /// permanent").
    JumpUnlessLkiYours { to: u16 },
    /// The controller may attach the source (an Equipment) to the token the
    /// last `CreateToken` made. Choice: yes/no.
    MayAttachToMade,
    /// Put each legal target of requirement `tgt` (a card in a graveyard)
    /// into its owner's hand, or onto the battlefield (under the
    /// ability's controller with `yours`, else its owner's), tapped if
    /// `tapped`. Remembers the mana value of the last one
    /// (`Amount::LkiManaValue`).
    FromGraveyard { tgt: u8, to_battlefield: bool, yours: bool, tapped: bool },
    /// Each player in APNAP order chooses a card matching `filter` in
    /// their graveyard (if any); then those cards are put onto the
    /// battlefield at once (CR 101.4). Choice: a `GRAVEYARD` slot.
    EachPutFromGraveyard { filter: Filter },
    /// Exile every card in `who`'s graveyard.
    ExileGraveyard { who: Who },
    /// Unless the spell was cast for an alternative cost, continue at `to`.
    JumpUnlessAlt { to: u16 },
    /// The source deals each legal target of requirement `to` its share of
    /// the divided damage (CR 601.2d).
    DealDivided { to: u8 },
    /// Each legal target of requirement `tgt` (a spell) goes on top of or
    /// on the bottom of its owner's library, as its owner chooses. Choice:
    /// `YES` = top, `PASS` = bottom.
    SpellToLibraryOwnerChooses { tgt: u8 },
    /// The controller sacrifices the source, if it is still the same
    /// permanent.
    SacrificeSource,
    /// Each object of `on` explores (CR 701.44): its controller reveals the
    /// top card of their library; a land goes into their hand; otherwise a
    /// +1/+1 counter on the creature, and they may put the card into their
    /// graveyard. Choice: yes/no.
    Explore { on: On },
}

/// What `Ext::Choose` does to the chosen permanents.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ChooseAct {
    Untap,
    Destroy,
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
    /// `who` shuffles their hand and/or graveyard into their library.
    ShuffleIntoLibrary { who: Who, hand: bool, graveyard: bool },
    /// `who` puts a card matching `filter` from their hand onto the
    /// battlefield (tapped if `tapped`); with `optional` they may not.
    /// Choice: a card in their hand.
    PutFromHand { who: Who, filter: Filter, tapped: bool, optional: bool },
    /// `whose` hand is revealed to everyone (`to: None`, CR 701.16) or
    /// looked at by `to`. Keys per card, in definition order.
    RevealHand { whose: Who, to: Option<Who> },
    /// `chooser` chooses a card matching `filter` from `whose`'s hand, which
    /// they have seen; that player discards it ("you choose a ... card from
    /// it; that player discards that card"). With `optional` the chooser
    /// may choose nothing.
    ChooseDiscard { chooser: Who, whose: Who, filter: Filter, optional: bool },
    /// The instructions of `Ext`.
    Ext(Ext),
}

impl Op {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        let amt = |a: &Amount| matches!(a, Amount::EventAmount);
        let who = |w: &Who| matches!(w, Who::EventPlayer);
        match self {
            Op::Ext(Ext::Apply { on, .. })
            | Op::Ext(Ext::ProtectionChoice { on, .. })
            | Op::Ext(Ext::Tap { on })
            | Op::Ext(Ext::Untap { on }) => matches!(on, On::EventObj),
            Op::Ext(Ext::PutCounters { on, n, .. }) => matches!(on, On::EventObj) || amt(n),
            Op::Ext(Ext::DoubleCounters { on, .. }) | Op::Ext(Ext::Explore { on }) => matches!(on, On::EventObj),
            Op::Ext(Ext::Choose { who: w, .. }) | Op::Ext(Ext::ExileGraveyard { who: w }) => who(w),
            Op::Ext(Ext::ApplyRules { who: w, .. }) | Op::Ext(Ext::RevealTopToHand { who: w }) => who(w),
            Op::Ext(Ext::Mill { who: w, n }) => who(w) || amt(n),
            Op::DealDamage { amount, .. } | Op::DamageAll { amount, .. } => amt(amount),
            Op::DamagePlayer { amount, who: w } => amt(amount) || who(w),
            Op::Draw { who: w, n } | Op::GainLife { who: w, n } | Op::LoseLife { who: w, n } | Op::DiscardRandom { who: w, n } => {
                who(w) || amt(n)
            }
            _ => false,
        }
    }

    /// The forward jump target of a control-flow op.
    fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            Op::Jump { to } | Op::JumpUnless { to, .. } | Op::JumpUnlessTarget { to, .. } => Some(to),
            Op::May { skip, .. } => Some(skip),
            Op::OfferPay { if_paid, .. } => Some(if_paid),
            Op::Ext(Ext::JumpUnlessSource { to, .. }) => Some(to),
            Op::Ext(Ext::DiscardThen { skip, .. }) => Some(skip),
            Op::Ext(Ext::FlipCoin { won }) => Some(won),
            Op::Ext(Ext::JumpUnlessLkiYours { to }) => Some(to),
            Op::Ext(Ext::JumpUnlessAlt { to }) => Some(to),
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
    /// A divided amount among the targets of a requirement (CR 601.2d).
    pub divide: Option<(u8, u8)>,
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
        AbilityDef { targets: self.targets, program: self.program, modes: None, divide: self.divide }
    }
}
