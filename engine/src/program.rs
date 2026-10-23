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
    /// The controller's devotion to a colour (index W U B R G; CR 700.5).
    Devotion(u8),
    /// The number of permanents of these card types the controller
    /// controls ("the number of lands you control").
    CountTypes(u16),
    /// The spell's X (CR 107.3).
    X,
    /// The power of the triggered ability's event object ("its power"):
    /// now if it is still on the battlefield, else as it last existed there
    /// (CR 608.2h).
    EventObjPower,
    /// The number of counters of a kind on the source (as it last existed
    /// on the battlefield if it left, CR 608.2h).
    SourceCounters(Counter),
    /// The number of the controller's opponents who lost life this turn.
    OpponentsLostLife,
    /// The number an earlier op of the program remembered (`Frame::x`): the
    /// cards `ChooseDiscard` had discarded ("if you do, ... then draws a
    /// card").
    Remembered,
    /// Half the cards in the controller's library, rounded up.
    HalfLibraryUp,
}

/// Which objects an `Ext` op acts on.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum On {
    /// Each legal target of requirement i.
    Target(u8),
    /// The object the last op made (a token, a card put onto the
    /// battlefield), if it is still that permanent.
    Made,
    /// The ability's source, if it is still the same object on the
    /// battlefield.
    Source,
    /// The object of the triggered ability's event ("that creature"), if
    /// it is still the same object on the battlefield.
    EventObj,
    /// Each permanent matching the filter at resolution ("creatures you
    /// control"; "you" is the controller, "another" excludes the source).
    Each(Filter),
    /// Every token the last `CreateToken` made that is still that
    /// permanent ("they gain haste").
    MadeAll,
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
    /// Exile each legal target of requirement `tgt`, linked to the source
    /// (CR 607.2a); with `until`, only while the source is on the
    /// battlefield, and they return when it leaves (CR 610.3).
    ExileLinked { tgt: u8, until: bool },
    /// `chooser` chooses a card matching `filter` from `whose`'s hand (which
    /// they have seen); it is exiled, linked to the source as above. With
    /// `optional` they may choose none. Choice: a card of the seen hand
    /// (`LIBRARY` slot) or `PASS`.
    ExileFromHandLinked { chooser: Who, whose: Who, filter: Filter, optional: bool, until: bool },
    /// The cards exiled with the source return to their owners' hands.
    ReturnLinkedToHand,
    /// Create delayed triggered ability `k` of the source's card (CR 603.7;
    /// its trigger `delayed_base + k`) for `when`, about `about`.
    Delay { k: u8, when: crate::db::DelayedWhen, about: About },
    /// Exile each legal target of requirement `tgt`; for each card exiled,
    /// create delayed triggered ability `k` about it (Flickerwisp).
    ExileThenDelay { tgt: u8, k: u8, when: crate::db::DelayedWhen },
    /// Put the card of the triggered ability's event (a card in exile or a
    /// graveyard, still the same object) onto the battlefield under its
    /// owner's control; it becomes the made object.
    ReturnEventCard,
    /// Unless the made object is a permanent the controller controls,
    /// continue at `to` ("if it entered under your control").
    JumpUnlessMadeYours { to: u16 },
    /// Sacrifice / exile the object of the event, if it is still the same
    /// permanent ("sacrifice the creature", "exile it").
    SacrificeEventObj,
    ExileEventObj,
    /// Add n {C} (`n` an amount).
    AddColorless { n: Amount },
    /// Remember the mana value of the first legal target spell of
    /// requirement `tgt` (`Amount::LkiManaValue`).
    RememberSpellMv { tgt: u8 },
    /// Put the top card matching `filter` of your graveyard onto the
    /// battlefield; it becomes the made object (Shallow Grave).
    TopOfGraveyardToBattlefield { filter: Filter },
    /// Each object of `on` explores (CR 701.44): its controller reveals the
    /// top card of their library; a land goes into their hand; otherwise a
    /// +1/+1 counter on the creature, and they may put the card into their
    /// graveyard. Choice: yes/no.
    Explore { on: On },
    /// Each object of `on` gets +p*N/+t*N until end of turn, N the number
    /// of permanents matching `filter` as it resolves (locked in, CR
    /// 608.2h).
    ApplyPtPerCount { on: On, filter: Filter, p: i8, t: i8 },
    /// The controller may tap or untap each object of `on`. Choice:
    /// `NUMBER` 0 = tap, 1 = untap; `PASS` = neither.
    TapOrUntap { on: On },
    /// `who` creates n tokens.
    CreateTokensFor { who: Who, token: u16, n: u8 },
    /// Create n tokens [tapped] and attacking (CR 508.4: not declared as
    /// attackers); outside combat, or for a player not attacking, they
    /// just enter (506.3).
    CreateTokensAttacking { token: u16, n: u8, tapped: bool },
    /// `who` puts all the cards from their graveyard on the bottom of
    /// their library in a random order.
    GraveyardToBottomRandom { who: Who },
    /// The rest of the looked-at pile goes on the bottom in a random order.
    PileRestBottomRandom,
    /// Damage `amount` to each legal target of requirement `tgt`, dealt by
    /// the event object ("that creature deals damage") if `event_obj`,
    /// else the source. With `no_lifegain`, a player dealt damage this way
    /// can't gain life for the rest of the game.
    DamageFrom { event_obj: bool, amount: Amount, tgt: u8, no_lifegain: bool },
    /// Put a +1/+1 counter on each object of `on`; if this is the second
    /// time this ability (of this source object) resolved this turn,
    /// double the +1/+1 counters on it instead.
    CounterOrDoubleOnSecond { on: On },
    /// "[You may] sacrifice <filter>. If you do, ...": the controller
    /// sacrifices a permanent they control matching `filter` (or, if
    /// `optional`, declines), else continues at `skip`. Choice: a
    /// permanent, or `PASS`.
    SacrificeThen { filter: Filter, optional: bool, skip: u16 },
    /// "You may remove a counter from this. When you do, ...": choose a
    /// kind of counter on the source to remove one of, or decline (or
    /// none: continue at `skip`). Choice: `NUMBER` k = the k-th kind, or
    /// `PASS`.
    MayRemoveCounterThen { skip: u16 },
    /// A reflexive triggered ability (CR 603.12): delayed ability `k` of
    /// the source's card triggers now, about the same event.
    Reflexive { k: u8 },
    /// Exile the top n cards of `who`'s library (revealed); the
    /// controller may play them (`kind`), this turn if `this_turn`.
    ExileTopWithPermission { who: Who, n: u8, kind: crate::game::PermKind, this_turn: bool },
    /// Each legal target of requirement `tgt` (a card in a graveyard)
    /// gains flashback until end of turn (its mana cost).
    GrantFlashback { tgt: u8 },
    /// The controller sacrifices the source unless it escaped.
    SacrificeSourceUnlessEscaped,
    /// Return the object of the event to its owner's hand, if it is still
    /// that permanent (dash).
    ReturnEventObjToHand,
    /// Amass (CR 701.47): if the controller controls no Army (`army`
    /// subtype), create `token` (a 0/0 Army); put n +1/+1 counters on an
    /// Army they control (their choice); it becomes a `subtype` too.
    Amass { token: u16, army: u16, subtype: u16, n: u8 },
    /// The card that died in the event ("return it"): if it is still that
    /// card in its owner's graveyard, it returns to the battlefield under
    /// its owner's control with a counter of this kind (persist).
    ReturnDiedWithCounter { counter: crate::db::Counter },
    /// For each card exiled with the source (CR 607.2a), its owner creates
    /// `token` with base power and toughness equal to the card's mana value.
    LinkedOwnersCreateMvToken { token: u16 },
    /// Monstrosity n (CR 701.37): unless the source is monstrous, put n
    /// +1/+1 counters on it; it becomes monstrous ("becomes monstrous"
    /// event).
    Monstrosity { n: u8 },
    /// Create `token`, then attach the source (an Equipment) to it (living
    /// weapon, For Mirrodin!).
    CreateTokenAttachSource { token: u16 },
    /// `who` looks at the top n cards of their library (a pile).
    LookTopN { who: Who, n: Amount },
    /// The controller wins the game (CR 104.2b).
    WinGame,
    /// Return all permanents to their owners' hands (at once).
    BounceAll,
    /// Will of the council: starting with the controller, each player
    /// votes for a permanent matching `filter` ("you" = the controller);
    /// exile each with the most votes or tied. Choice: a permanent.
    VoteExile { filter: Filter },
    /// `who` mills two cards; if two cards that share a colour were milled,
    /// repeat (Grindstone).
    MillTwoWhileShareColor { who: Who },
    /// Unless the controller pays the made object's mana cost reduced by
    /// {n}, sacrifice it (Flash). Choice: yes/no, then `PayMana`.
    SacMadeUnlessPayReduced { n: u8 },
    /// `who` may reveal cards from the top of their library until a card
    /// of these types; it is put onto the battlefield under their control,
    /// the others into their graveyard (Oath of Druids). Choice: yes/no.
    RevealUntilToBattlefield { who: Who, types: u16 },
    /// The controller gets n energy counters.
    GetEnergy { n: u8 },
    /// The controller may pay any amount of energy; the source deals that
    /// much damage to each legal target of `tgt`. Choice: `NUMBER`.
    PayEnergyDamage { tgt: u8 },
    /// "You may pay {E} x n. When you do, ...": unless paid, continue at
    /// `skip`. Choice: yes/no.
    MayPayEnergyThen { n: u8, skip: u16 },
    /// The rest of the pile is exiled; the controller may play those cards
    /// this turn.
    PileRestExilePlayable,
    /// The controller looks at the top card of `who`'s library.
    LookAtTopOf { who: Who },
    /// Put each legal target of requirement `tgt` into its owner's library
    /// beneath the top X cards.
    PutInLibraryAtX { tgt: u8 },
    /// Sacrifice the source if it is still the same permanent, else
    /// continue at `skip` ("you may sacrifice it. If you do").
    SacrificeSourceThen { skip: u16 },
    /// "You may mill that many cards. If you do, you may put a creature
    /// card from among them into your hand" (the event's amount). Choice:
    /// yes/no, then a `GRAVEYARD` slot or `PASS`.
    MayMillThenTakeCreature,
    /// Each creature card exiled with the source (CR 607.2a) is put onto
    /// the battlefield under the controller's control [with a flying
    /// counter] [as a p/t <subtype> in addition to its other types].
    LinkedCreaturesToBattlefield { flying: bool, becomes: Option<(u16, i32, i32)> },
    /// The controller looks at a card at random in `who`'s hand.
    LookAtRandomHandCard { who: Who },
    /// Reveal the top n cards; for each card type, the controller may put a
    /// card of that type from among them into their hand (each card for
    /// one type); the rest go on the bottom in a random order. Choice: a
    /// `LIBRARY` slot or `PASS`.
    RevealTopOnePerType { n: u8 },
    /// Create a token that's a copy of the first legal target of `tgt` (a
    /// token: its definition, CR 707.2).
    CopyTargetToken { tgt: u8 },
    /// An opponent gains control of the source (CR 108.4).
    GiveSourceToOpponent,
    /// Exile the event's second object ("that creature"), if it is still
    /// that permanent.
    ExileEventObj2,
    /// The controller becomes the monarch (CR 724).
    BecomeMonarch,
    /// Exile each legal target of `tgt` until an opponent of the controller
    /// becomes the monarch (Palace Jailer).
    ExileUntilMonarch { tgt: u8 },
    /// Exile this spell (Time Spiral).
    ExileThisSpell,
    /// Reveal cards from the top of the controller's library until a
    /// creature card; it enters tapped and attacking (508.4), the rest go
    /// to the bottom in a random order.
    RevealUntilCreatureAttacking,
    /// Unless the source has no counters of this kind (now, or as it last
    /// existed on the battlefield), continue at `to`.
    JumpUnlessSourceNoCounters { counter: crate::db::Counter, to: u16 },
    /// The controller may cast each legal target of `tgt` (a card in
    /// their graveyard) this turn.
    GraveyardCastPermission { tgt: u8 },
    /// A "whenever you create one or more creature tokens" event for the
    /// controller (after an effect that may have created some: emitted if
    /// it did, `made` set).
    TokensCreated,
    // ---- M7b: planeswalkers and their effects (`resolve_walkers.rs`) ----
    /// The controller gets emblem `emblem` (a token-table definition, CR
    /// 114.2).
    CreateEmblem { emblem: u16 },
    /// Remove n counters of a kind from each object of `on` (as many as
    /// it has).
    RemoveCounters { on: On, counter: Counter, n: Amount },
    /// The controller gains control of each legal target of `tgt` (CR
    /// 613.1b, no duration).
    GainControl { tgt: u8 },
    /// Exchange control of the first legal targets of `a` and `b` (CR
    /// 701.12a-b: if either is gone, nothing happens).
    ExchangeControl { a: u8, b: u8 },
    /// The controller gains control of the permanents matching `filter`
    /// that the event's spell targets (as it is on the stack).
    GainControlOfEventSpellTargets { filter: Filter },
    /// The controller rolls a `sides`-sided die (CR 706; a public chance
    /// key): the first `(lo, hi, pc)` entry whose range holds the result
    /// continues at `pc`; none: the next op.
    RollDie { sides: u8, table: Vec<(u8, u8, u16)> },
    /// The source's controller may activate its loyalty abilities `n` more
    /// times this turn (CR 606.3 exception).
    ExtraLoyaltyActivations { n: u8 },
    /// The controller chooses a card matching `filter` in their graveyard
    /// (not targeted) and puts it into their hand; nothing if none. Choice:
    /// a `GRAVEYARD` slot.
    ChooseFromGraveyardToHand { filter: Filter },
    /// The source deals `amount` damage to a creature or player the
    /// controller chooses as it resolves (not targeted). Choice: `PLAYER` or
    /// a permanent's slot (a `Target` decision).
    DamageChosen { amount: Amount },
    /// Exile the top card of the controller's library (revealed); they may
    /// cast it now (CR 608.2g: during resolution, ignoring the timing
    /// permissions of its card type). If they do, continue at `skip`.
    /// Choice: yes/no.
    ExileTopMayCast { skip: u16 },
    /// Exile every card of `who`'s library (face up).
    ExileLibrary { who: Who },
    /// The controller looks at the top card of `who`'s library and may put
    /// it on the bottom. Choice: yes/no.
    LookAtTopMayBottom { who: Who },
    /// Ninjutsu's effect (CR 702.49a): the source card, if still in its
    /// owner's hand, is put onto the battlefield tapped and attacking what
    /// the returned creature attacked (702.49c).
    NinjutsuEnter,
    /// Reveal the top n cards of the controller's library; an opponent
    /// chooses one of them, which goes into the controller's hand; the rest
    /// are exiled with a counter of this kind on them. Choice (the
    /// opponent's): a `LIBRARY` slot of the revealed cards.
    RevealTopOpponentChooses { n: u8, counter: Counter },
    /// The controller puts a card they own in exile with a counter of this
    /// kind into their hand. Choice: `EXILE` + k.
    ExiledWithCounterToHand { counter: Counter },
    /// The controller sacrifices a permanent matching `filter` (their
    /// choice; nothing if none). When they do (CR 603.12), delayed ability
    /// `k` of the source's card triggers, about the sacrificed object and
    /// its power as it last existed (`EventAmount`).
    SacrificeThenReflexive { filter: Filter, k: u8 },
    /// Unless the event's object (in any zone, by its characteristics there)
    /// matches `filter`, continue at `to`.
    JumpUnlessEventObj { filter: Filter, to: u16 },
    /// Search the controller's library for any number of cards matching
    /// `filter` (one decision per card, `PASS` = done), then put them onto
    /// the battlefield (tapped with `tapped`), or exile them with a
    /// permission to cast them this turn without paying their mana costs
    /// (`exile_free`); then shuffle (CR 701.19). Choice: a definition.
    SearchAnyNumber { filter: Filter, tapped: bool, exile_free: bool },
    /// The controller names a card (a nonland one with `nonland`), then
    /// reveals the top n cards of their library; the ones with that name go
    /// into their hand, the rest into their graveyard. Choice: `NameCard`.
    NameRevealTopToHand { n: u8, nonland: bool },
    /// "Until your next turn, you may cast sorcery spells as though they
    /// had flash."
    FlashSorceries,
    // ---- M7b part 2: cards with more than one face (`faces.rs`) ----
    /// Transform the source (CR 701.27), if it is still that permanent and
    /// hasn't transformed since this ability was put on the stack
    /// (701.27f).
    TransformSource,
    /// Exile the source, if it is still that permanent, then return it to
    /// the battlefield transformed (CR 712.14a) under its owner's control
    /// (or the controller's, `yours`); a card that isn't double-faced stays
    /// in exile.
    ExileReturnTransformed { yours: bool },
    /// "You may [do this]. Do this only once each turn": like `Op::May`,
    /// unless this ability of this source was done this turn (then
    /// continue at `skip`, as if declined; CR 603.2h: once it is done, the
    /// ability doesn't trigger again that turn, `triggers.rs`).
    OnceMay { skip: u16 },
    /// The controller may cast each legal target of `tgt` (a card in their
    /// graveyard) this turn; if that spell would be put into a graveyard,
    /// it is exiled instead.
    GraveyardCastThenExile { tgt: u8 },
    /// Until the controller's next turn, they have triggered ability `k` of
    /// the source's card (its trigger `delayed_base + k`, with its own
    /// event); it may trigger any number of times.
    TriggerUntilYourNextTurn { k: u8 },
    /// Each player in APNAP order exiles cards from the top of their
    /// library until they exile a nonland card (revealed); the controller
    /// may cast any number of those nonland cards without paying their
    /// mana costs (CR 608.2g), one decision per card. Choice: yes/no.
    EachExileUntilNonlandCastFree,
    /// `who` gets n poison counters (CR 122.1f).
    Poison { who: Who, n: Amount },
    /// The controller may discard up to n cards (one decision per card,
    /// `PASS` = no more), then draws as many as they discarded.
    DiscardUpToThenDraw { n: u8 },
    /// The controller's opponent chooses, for each card type in `types` in
    /// order, a [nonland] permanent of that type they control (one decision
    /// each; a permanent may be chosen for several types), then sacrifices
    /// the other [nonland] permanents they control at once. Choice: a
    /// permanent.
    KeepOneOfEachType { types: Vec<u16>, nonland: bool },
    /// The source connives (CR 701.50a): its controller draws a card, then
    /// discards a card (a decision); if a nonland card was discarded, a
    /// +1/+1 counter on the source if it is still that permanent.
    Connive,
}

/// What a delayed trigger is about ("that card", "that much").
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum About {
    Nothing,
    /// The object the last `CreateToken` or put-onto-the-battlefield made.
    Made,
    /// The number remembered (`Amount::LkiManaValue`).
    Remembered,
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
        let amt = |a: &Amount| matches!(a, Amount::EventAmount | Amount::EventObjPower);
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
            Op::Ext(Ext::ReturnEventCard) | Op::Ext(Ext::SacrificeEventObj) | Op::Ext(Ext::ExileEventObj) => true,
            Op::Ext(Ext::AddColorless { n }) => amt(n),
            Op::Ext(Ext::DamageFrom { event_obj, amount, .. }) => *event_obj || amt(amount),
            Op::Ext(Ext::Reflexive { .. })
            | Op::Ext(Ext::ReturnEventObjToHand)
            | Op::Ext(Ext::MayMillThenTakeCreature)
            | Op::Ext(Ext::ExileEventObj2) => true,
            Op::Ext(Ext::ReturnDiedWithCounter { .. }) => true,
            Op::Ext(Ext::JumpUnlessEventObj { .. }) | Op::Ext(Ext::GainControlOfEventSpellTargets { .. }) => true,
            Op::Ext(Ext::ExileTopWithPermission { who: w, .. }) => who(w),
            Op::Ext(Ext::Poison { who: w, n }) => who(w) || amt(n),
            Op::Ext(Ext::ApplyPtPerCount { on, .. })
            | Op::Ext(Ext::TapOrUntap { on })
            | Op::Ext(Ext::CounterOrDoubleOnSecond { on }) => matches!(on, On::EventObj),
            Op::Ext(Ext::CreateTokensFor { who: w, .. }) | Op::Ext(Ext::GraveyardToBottomRandom { who: w }) => who(w),
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
            Op::Ext(Ext::JumpUnlessMadeYours { to }) => Some(to),
            Op::Ext(Ext::SacrificeThen { skip, .. }) => Some(skip),
            Op::Ext(Ext::MayRemoveCounterThen { skip }) => Some(skip),
            Op::Ext(Ext::MayPayEnergyThen { skip, .. }) => Some(skip),
            Op::Ext(Ext::SacrificeSourceThen { skip }) => Some(skip),
            Op::Ext(Ext::JumpUnlessSourceNoCounters { to, .. }) => Some(to),
            Op::Ext(Ext::ExileTopMayCast { skip }) => Some(skip),
            Op::Ext(Ext::JumpUnlessEventObj { to, .. }) => Some(to),
            Op::Ext(Ext::OnceMay { skip }) => Some(skip),
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
    pub divide: Option<(u8, crate::db::DivAmount)>,
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
