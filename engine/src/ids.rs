//! Identifiers for players, cards and objects (ENGINE.md 2).

/// A player: 0 or 1.
pub type PlayerId = u8;

/// Index of a card definition in the [`CardDb`](crate::db::CardDb).
pub type DefId = u16;

/// Index of a card slot in `Game.objs`. One slot per physical card for the
/// whole match; tokens and copies will append slots. Never reused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CardIdx(pub u16);

impl CardIdx {
    #[inline]
    pub fn i(self) -> usize {
        self.0 as usize
    }
}

/// An object (CR 109): a card slot in one incarnation. Every zone change
/// bumps the slot's incarnation (CR 400.7), so a stale reference is
/// detected by comparing `inc`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ObjRef {
    pub idx: CardIdx,
    pub inc: u16,
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
}

impl Zone {
    /// Hidden zones (CR 400.2): a card's identity there is known only to
    /// the players whose `known` bit is set.
    pub fn is_hidden(self) -> bool {
        matches!(self, Zone::Library | Zone::Hand)
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
