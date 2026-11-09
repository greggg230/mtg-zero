//! Identifiers for players, cards and objects (ENGINE.md 2).

/// A player: 0 or 1.
pub type PlayerId = u8;

/// Index of a card definition in the [`CardDb`](crate::db::CardDb).
pub type DefId = u16;

/// Index of a card slot in `Game.objs`. One slot per physical card for the
/// whole match; tokens and copies append slots. Never reused. (`u32`: a game
/// runs out of memory long before 2^32 objects; `Game::add_card` aborts the
/// game near the bound, ENGINE.md 100.7.)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct CardIdx(pub u32);

impl CardIdx {
    #[inline]
    pub fn i(self) -> usize {
        self.0 as usize
    }
}

/// An object (CR 109): a card slot in one incarnation. Every zone change
/// bumps the slot's incarnation (CR 400.7), so a stale reference is
/// detected by comparing `inc` (`u32`, counted exactly: a slot's 2^32nd zone
/// change aborts the game, `Game::move_card`, so an old reference never
/// aliases a new object).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ObjRef {
    pub idx: CardIdx,
    pub inc: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Zone {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Stack,
    Exile,
    /// Tokens that ceased to exist, cards not yet in the game.
    Nowhere,
    /// The command zone (CR 408): emblems (CR 114).
    Command,
    /// Exiled face down (CR 406.3): the exile zone, but the card's identity
    /// is hidden like a library card's (known to the players whose `known`
    /// bit is set: its owner, who saw it go there; ENGINE.md 10, 30).
    FaceDown,
    /// On the battlefield, phased out (CR 702.26b): treated as though it
    /// doesn't exist; it hasn't changed zones (702.26d: the same object, its
    /// counters and its continuous effects kept). `Game::w3`'s list. EoE
    /// W3, ENGINE.md 99.13.
    PhasedOut,
}

impl Zone {
    /// Hidden zones (CR 400.2): a card's identity there is known only to
    /// the players whose `known` bit is set.
    pub fn is_hidden(self) -> bool {
        matches!(self, Zone::Library | Zone::Hand | Zone::FaceDown)
    }
}

/// A chosen target: a player, an object (a permanent, a spell on the stack, a
/// card in a graveyard), or an activated or triggered ability on the stack
/// (CR 115.1, 113.1a: by its stack item's id; an ability is an object on the
/// stack, but not a card's).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Target {
    Player(PlayerId),
    Obj(ObjRef),
    Stack(u32),
}

/// The other player (2p).
#[inline]
pub fn opp(p: PlayerId) -> PlayerId {
    1 - p
}
