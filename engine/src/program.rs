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
    /// The player chosen for target requirement `i`, or the controller of
    /// the object chosen for it ("that player or that permanent's
    /// controller").
    TargetOrItsController(u8),
    /// The player other than the controller of the object last remembered
    /// ("each player other than its controller", 2p).
    NotLkiController,
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
    /// The times the additional cost of the event's permanent was paid as
    /// its spell was cast (squad, CR 702.157a: the `Enters` event's note).
    EventTimes,
    /// The times the additional cost of the event's spell was paid
    /// (replicate, CR 702.56a).
    EventSpellTimes,
    /// The number of attacking creatures (CR 506.4).
    Attacking,
    /// The controller's experience counters (CR 122.1).
    Experience,
    /// The mana value of the triggered ability's event spell ("that
    /// spell's mana value"): on the stack now (with its X, CR 202.3e), or as
    /// it last existed there (608.2h).
    EventSpellMv,
    /// n plus the mana value of the permanent sacrificed to pay the
    /// activated ability's cost ("2 plus the sacrificed permanent's mana
    /// value"; as it last existed, CR 608.2h).
    CostMvPlus(u8),
    /// The power of the permanent sacrificed to pay the activated ability's
    /// cost, as it last existed on the battlefield ("draw cards equal to the
    /// sacrificed creature's power", CR 608.2h; the item's event object,
    /// recorded as the cost is paid).
    CostPower,
    /// The power of the object chosen for the spell's additional cost
    /// (`StackItem::noted`, ENGINE.md 99.14): a creature on the battlefield
    /// now, else as it last existed there; a card in exile, its own (CR
    /// 608.2h: as it last existed in exile if it left).
    ChosenPower,
    /// k times the spell's X ("twice X").
    XTimes(u8),
    /// The number of permanents of these card types the controller's
    /// opponent controls ("the number of artifacts they control").
    CountTypesOpp(u16),
    /// k times [`CountTypes`](Amount::CountTypes) ("twice the number of
    /// lands you control").
    CountTypesTimes(u16, u8),
    /// The controller's life total ("cards equal to your life total").
    ControllerLife,
    /// The multicolored permanents the controller controls (CR 105.2a).
    MulticoloredPermanents,
    /// The differently named lands the controller controls (CR 201.2).
    DifferentlyNamedLands,
    /// The power of the creature tapped to pay the activated ability's cost
    /// (the stack item's first noted object): station's "a number of charge
    /// counters equal to the tapped creature's power" (CR 702.184a), read as
    /// the ability resolves, or as the creature last existed on the
    /// battlefield if it left (608.2h; station's ruling). Its toughness
    /// instead where a static ability says so (702.184c).
    TappedPower,
}

/// The instructions Edge of Eternities lane B added (ENGINE.md 99.3), inside
/// `Ext::EoeB`. Same rules as `Op`: named after rules actions, never cards.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeB {
    /// Count this resolution of the ability (of this source object, as
    /// `CounterOrDoubleOnSecond`); unless it is the second time it resolved
    /// this turn, continue at `to` ("if this is the second time this ability
    /// has resolved this turn").
    JumpUnlessSecondResolution { to: u16 },
    /// The source deals `amount` damage to the first legal target of `tgt`
    /// (a creature); unless excess damage was dealt (CR 120.10: damage
    /// beyond lethal damage, 120.6 / 702.2c), continue at `to`.
    DamageJumpUnlessExcess { amount: Amount, tgt: u8, to: u16 },
    /// The controller exiles cards from the top of their library until they
    /// exile a nonland card (CR 701.20a: each revealed); they may cast that
    /// card this turn.
    ExileUntilNonlandMayCastThisTurn,
}

impl EoeB {
    fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeB::JumpUnlessSecondResolution { to } | EoeB::DamageJumpUnlessExcess { to, .. } => Some(to),
            EoeB::ExileUntilNonlandMayCastThisTurn => None,
        }
    }
}

/// The instructions of the Edge of Eternities port's lane D (ENGINE.md
/// 99.5), inside `Ext::EoeD`. Same rules as `Op`: named after rules actions,
/// never cards.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeD {
    /// The first legal target of requirement `by` (a permanent) deals damage
    /// equal to its power to each legal target of requirement `to`, or to
    /// itself (`None`); twice that much if it matches `twice_if` (CR 120.1,
    /// 120.3). An illegal `by` deals none (608.2b).
    PowerDamageByTarget { by: u8, to: Option<u8>, twice_if: Option<Filter> },
    /// Remember (`Frame::x`, `Amount::Remembered`) `plus` plus the number of
    /// permanents the controller controls matching `filter`.
    CountControlled { filter: Filter, plus: i32 },
    /// Each object of `on` gets +p*n/+t*n until `until`, n read once as the
    /// op resolves (CR 608.2h, 611.2c; "double its power", 701.10b).
    ApplyPtTimes { on: On, p: i8, t: i8, n: Amount, until: Until },
    /// The triggered ability's event card (in a graveyard or exile) returns
    /// to the battlefield tapped under its owner's control ("return it to
    /// the battlefield tapped": it enters tapped and never "becomes
    /// tapped").
    ReturnEventCardTapped,
    /// `who`'s life total becomes half their starting life total, rounded
    /// up: they gain or lose the difference (CR 119.5, 107.1a).
    LifeBecomesHalfStarting { who: Who },
    /// The controller may tap an untapped permanent they control matching
    /// `filter`; if they don't, the source is sacrificed ("sacrifice it
    /// unless you tap ...", CR 118.12, 701.21a). Choice: `ChoosePermanent`,
    /// `PASS` = don't.
    SacrificeSourceUnlessTap { filter: Filter },
    /// Each legal target player of requirement `tgt` mills half their
    /// library, rounded down (CR 701.17a, 107.1a).
    MillHalfLibrary { tgt: u8 },
    /// `who` loses half their life, rounded up (CR 119.3, 107.1a).
    LoseHalfLife { who: Who },
    /// Destroy all creatures with toughness X or less, X the spell's (CR
    /// 701.8a, 107.3); remember (`Frame::picked`) the cards put into a
    /// graveyard this way.
    DestroyCreaturesToughnessAtMostX,
    /// If the spell's X is n or more, the controller returns one of the
    /// cards `DestroyCreaturesToughnessAtMostX` put into a graveyard (still
    /// there) to the battlefield under their control. Choice:
    /// `ChooseGraveyardCard`.
    ReturnOneDestroyedIfXAtLeast { n: u8 },
    /// The controller gains control of each object of `on` until end of
    /// turn (a control-changing effect with a duration, layer 2: CR 613.1b,
    /// 611.2a; it ends in the cleanup step, 514.2, `Mod::ControlFrom`).
    GainControlUntilEndOfTurn { on: On },
    /// The controller chooses odd or even; destroy each creature whose mana
    /// value has the chosen quality (zero is even; CR 701.8a). Choice:
    /// `ChooseMode`: `NUMBER` 0 = even, 1 = odd (as "choose odd or even" as a
    /// permanent enters).
    DestroyCreaturesOfChosenParity,
    /// Delayed triggered ability `k` (CR 603.7), about the first legal target
    /// of requirement `tgt` ("remove ten charge counters from it at the
    /// beginning of the next end step").
    DelayAboutTarget { k: u8, when: crate::db::DelayedWhen, tgt: u8 },
}

impl EoeD {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeD::ApplyPtTimes { on, n, .. } => {
                matches!(on, On::EventObj)
                    || matches!(n, Amount::EventAmount | Amount::EventObjPower | Amount::EventTimes | Amount::EventSpellTimes | Amount::EventSpellMv)
            }
            EoeD::ReturnEventCardTapped => true,
            EoeD::LifeBecomesHalfStarting { who } => *who == Who::EventPlayer,
            EoeD::LoseHalfLife { who } => *who == Who::EventPlayer,
            EoeD::GainControlUntilEndOfTurn { on } => matches!(on, On::EventObj),
            EoeD::DestroyCreaturesOfChosenParity | EoeD::DelayAboutTarget { .. } => false,
            EoeD::PowerDamageByTarget { .. }
            | EoeD::CountControlled { .. }
            | EoeD::SacrificeSourceUnlessTap { .. }
            | EoeD::MillHalfLibrary { .. }
            | EoeD::DestroyCreaturesToughnessAtMostX
            | EoeD::ReturnOneDestroyedIfXAtLeast { .. } => false,
        }
    }
}

/// The instructions M7d added (ENGINE.md 31), inside `Ext::M7d`. Same rules
/// as `Op`: named after rules actions, never cards.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum M7d {
    /// Choose a card in exile an opponent owns with a counter of this kind;
    /// the controller may play it this turn without paying its mana cost.
    /// Choice: `EXILE` + k (Dauthi Voidwalker).
    ExiledWithCounterPlayFree { counter: Counter },
    /// Exile target card `tgt` (in a graveyard); if it was a permanent card,
    /// put a +1/+1 counter on the source (Lion Sash).
    ExileTargetCounterIfPermanent { tgt: u8 },
    /// The source becomes unattached (reconfigure, CR 702.151a).
    UnattachSource,
    /// The source (a Class) becomes level `level` (CR 716.2a): its face of
    /// that level is turned up.
    GainClassLevel { level: u8 },
    /// Each legal target of requirement `tgt` that isn't a creature becomes
    /// a p/t creature with this subtype in addition to its other types
    /// (forever).
    BecomeCreatureIfNot { tgt: u8, p: i32, t: i32, subtype: u16 },
    /// The controller reveals the top card of their library: a land card is
    /// put onto the battlefield, anything else into their hand (Nadu).
    RevealTopLandElseHand,
    /// The controller shuffles their library, then exiles its top card; they
    /// may play it this turn without paying its mana cost (Urza).
    ShuffleExileTopPlayFree,
    /// The target opponent (`tgt`) exiles the top card of their library, a
    /// card at random from their graveyard and one at random from their
    /// hand; the controller may cast a spell from among them without paying
    /// its mana cost (Crabomination). Choice: `ChooseExiled` (`PASS` = no).
    OppExilesThreeMayCastOne { tgt: u8 },
    /// Manifest dread (CR 701.62a). Choice: `PutOntoBattlefield`.
    ManifestDread,
    /// The resolving spell (the source, still on the stack) is shuffled into
    /// its owner's library as an instruction of its own effect (Green Sun's
    /// Zenith; then it isn't put into a graveyard, CR 608.2n). A spell cast
    /// with flashback is exiled instead (702.34a, `move_card`).
    ShuffleThisSpellIntoLibrary,
    /// The controller gets triggered ability `k` (a delayed one of the
    /// source definition, `delayed_base + k`) for the rest of the turn:
    /// "whenever <event> this turn, <effect>" (a delayed triggered ability
    /// with a duration, CR 603.7b; Forth Eorlingas!).
    TriggerThisTurn { k: u8 },
    /// "Whenever that creature <event> this turn, <effect>": the delayed
    /// triggered ability `k` (CR 603.7b, 603.7c), bound to each object of
    /// `on` (Okoye: "whenever that creature attacks the monarch this turn").
    TriggerThisTurnFor { k: u8, on: On },
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
    FromGraveyard { tgt: u8, to_battlefield: bool, yours: bool, tapped: bool, counter: Option<crate::db::Counter> },
    /// Each player in APNAP order chooses a card matching `filter` in
    /// their graveyard (if any); then those cards are put onto the
    /// battlefield at once (CR 101.4). Choice: a graveyard card (its slot
    /// in the graveyard list, ENGINE.md 65.10).
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
    /// the source's card triggers now, about the same event; with
    /// `per_token`, once for each token the preceding `CreateToken` made
    /// ("create a token. When you do, ..." doubled by Elspeth: one trigger
    /// per token, Ajani, Nacatl Avenger's ruling 47eed49e).
    Reflexive { k: u8, per_token: bool },
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
    /// yes/no, then a graveyard card (its graveyard list slot) or `PASS`.
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
    /// An opponent gains control of the source (CR 108.4).
    GiveSourceToOpponent,
    /// Exile the event's second object ("that creature"), if it is still
    /// that permanent.
    ExileEventObj2,
    /// The controller becomes the monarch (CR 724).
    BecomeMonarch,
    /// "Its controller becomes the monarch": the controller of the event's
    /// creature (its last known information if it left), the monarch's
    /// inherent triggered ability (CR 724.2).
    EventControllerBecomesMonarch,
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
    /// a graveyard card (its graveyard list slot, ENGINE.md 65.10).
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
    /// The M7d instructions (ENGINE.md 31).
    M7d(M7d),
    /// The EoE lane D instructions (ENGINE.md 99.5).
    EoeD(EoeD),
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
    /// The first object of `on` connives n (CR 701.50a): its controller
    /// draws n cards, then discards n cards (a decision each); for each
    /// nonland card discarded, a +1/+1 counter on it if it is still that
    /// permanent. With no object (it left), the ability's controller does
    /// it (701.50c-like, last known information).
    Connive { on: On, n: Amount },
    // ---- M7b part 3: copies (`copies.rs`, ENGINE.md 26) ----
    /// `who` creates n tokens that are copies of `of` (CR 707.2, 111.10
    /// copiable values), with the exceptions `exc` (707.9); they become the
    /// made objects. Nothing if `of` names no object (111.12).
    CreateTokenCopy { of: CopyOf, who: Who, n: Amount, exc: crate::copies::Except },
    /// Each object of `on` becomes a copy of `of` with the exceptions `exc`
    /// until `until` (CR 707.2, 613.2a; 707.4: it stays the same object).
    BecomeCopy { on: On, of: CopyOf, exc: crate::copies::Except, until: Until },
    /// `who` [may] put n copies of the spell `of` onto the stack (CR
    /// 707.10: not cast; modes, X, targets and costs copied), choosing new
    /// targets for each if `new_targets` (707.10c). Choice: yes/no if `may`.
    CopySpell { of: SpellOf, who: Who, n: Amount, may: bool, new_targets: bool },
    /// Exile the card of the triggered ability's event, if it is still that
    /// card in a graveyard ("exile that card").
    ExileEventCard,
    /// Each object of `on` becomes prepared (CR 722.3a).
    BecomePrepared { on: On },
    // ---- M7b part 3b: the stack, ward, the initiative, graveyard
    // abilities (`stack_ext.rs`, `dungeon.rs`; ENGINE.md 27) ----
    /// The source of each legal target ability of requirement `tgt`, if it
    /// is an artifact, creature or planeswalker on the battlefield (the
    /// same object), loses all abilities for as long as this ability's
    /// source remains on the battlefield (CR 611.2b; "if an ability of an
    /// artifact, creature, or planeswalker is countered this way, that
    /// permanent loses all abilities": right before the counter).
    SourceOfTargetLosesAbilities { tgt: u8 },
    /// The controller gains control of each legal target spell of `tgt`
    /// (a permanent spell then enters under their control, CR 608.3a) and
    /// may choose new targets for it (115.7).
    GainControlOfSpell { tgt: u8 },
    /// Counter the spell or ability of the triggered ability's event, if
    /// it is still on the stack ("counter that spell or ability": ward, CR
    /// 702.21a).
    CounterEventItem,
    /// `who` may pay `life` life (CR 119.4: only with that much life) or
    /// discard a card of their choice (`discard`); if they do, continue at
    /// `if_paid` ("unless that player pays", 118.12). Choice: yes/no, or a
    /// hand card / `PASS`.
    OfferPayOther { who: Who, life: u8, discard: bool, if_paid: u16 },
    /// `who` takes the initiative (CR 725.2: the initiative's "whenever a
    /// player takes the initiative, that player ventures into Undercity"
    /// triggers).
    TakeInitiative { who: Who },
    /// `who` ventures into Undercity (CR 701.49, 309.5). Choice: which room
    /// next (`ChooseRoom`).
    Venture { who: Who },
    /// The controller puts a card matching `filter` from among the pile
    /// onto the battlefield (their choice; nothing if none); it becomes the
    /// made object. Choice: a `LIBRARY` slot of the pile.
    PileToBattlefield { filter: Filter, counters: Option<(crate::db::Counter, u8)> },
    /// The controller gets n experience counters (CR 122.1).
    GetExperience { n: u8 },
    /// The controller creates n tokens (an amount) [tapped] [and attacking,
    /// CR 508.4].
    CreateTokensN { token: u16, n: Amount, tapped: bool, attacking: bool },
    /// The controller discards `discard` cards (a decision each), then
    /// draws `draw`; the nonland cards discarded are remembered
    /// (`Amount::Remembered`), or with `types` the number of card types
    /// among the cards discarded (Occult Epiphany, CR 205.2a).
    DiscardThenDraw { discard: Amount, draw: u8, types: bool },
    /// The source card, if it is still that card in its owner's graveyard,
    /// returns to the battlefield under its owner's control [tapped].
    SourceFromGraveyard { tapped: bool },
    // ---- M7c: the single-card tail (`resolve_tail.rs`, ENGINE.md 28) ----
    /// Remove every counter of a kind from the source (if it is still on
    /// the battlefield); the controller adds one mana of any colour for
    /// each (CR 106.4). Choice: a colour per mana (`ChooseColor`), in
    /// non-decreasing order (each multiset once).
    RemoveAllCountersAddMana { counter: Counter },
    /// The player of requirement `tgt` may have the controller draw a card;
    /// if they don't, the controller mills X cards (X = the source's
    /// counters of `counter`, as it last existed) and that player loses
    /// life equal to the total mana value of the cards milled. Choice (that
    /// player's): yes/no.
    LetDrawElseMill { tgt: u8, counter: Counter },
    /// Exile a card at random from the controller's graveyard (a public
    /// chance key); they may play it this turn.
    ExileRandomGraveyardPlayable,
    /// The controller names a card, exiles the top n cards of their library,
    /// then reveals cards from the top until one with that name: it goes
    /// into their hand, every other card revealed is exiled. Choice:
    /// `NameCard` (`PASS`: a name of no card in their library).
    NameExileRevealUntil { n: u8 },
    /// Balance: for lands, then cards in hand, then creatures, each player
    /// who has more than the player with the fewest sacrifices (discards)
    /// the excess of their choice (APNAP; a permanent per decision in
    /// increasing slot order, a card per decision discarded at once).
    Balance,
    /// Mill n cards; the source attacks the opponent this combat if able
    /// (2 players: "an opponent chosen at random" is that one); until end
    /// of turn it gains `kws` and gets +1/+1 for each card type among cards
    /// in the controller's graveyard (counted as it resolves).
    MillAttackPump { n: u8, kws: crate::db::KwSet },
    /// Sacrifice an artifact (a choice; none: nothing more). If they do,
    /// search for an artifact card: with mana value at most the sacrificed
    /// one's it is put onto the battlefield; with more, the controller may
    /// pay the difference to put it onto the battlefield, else it goes to
    /// the graveyard. Then shuffle. Choices: `Sacrifice`, `SearchLibrary`,
    /// yes/no then `PayMana`.
    SacrificeTutorPayDifference,
    /// Look at the top n cards; any number of times pay `life` life, put
    /// them on the bottom and look at the next n; then shuffle the rest of
    /// the library (the looked-at cards stay the pile, put on top by the
    /// next op). Choice: yes/no per round.
    LookTopRepeatPayLife { n: u8, life: u8 },
    /// Draw a card, then discard a card (a choice); if the source has at
    /// least n counters of `counter` (as it last existed), the controller
    /// may cast the discarded card without paying its mana cost (CR
    /// 608.2g). Choices: `Discard`, yes/no.
    LootMayCastFree { counter: Counter, n: u8 },
    /// The controller chooses a card matching `filter` from `whose`'s hand
    /// (which they have seen) or may choose none; it is exiled and its owner
    /// may play it for as long as it stays exiled, a spell cast this way
    /// costing {tax} more (Elite Spellbinder). Choice: a card of the seen
    /// hand (`LIBRARY` slot) or `PASS`.
    ExileFromHandTaxed { whose: Who, filter: Filter, tax: u8 },
    /// The controller may exile a card matching `filter` from `whose`'s
    /// hand (seen) or the first legal target of requirement `tgt` (a
    /// permanent), until the source leaves the battlefield (CR 610.3; Cloak
    /// and Dagger). Choice: a card of the seen hand (`LIBRARY` slot), the
    /// permanent's slot, or `PASS`.
    ExileHandCardOrTargetUntil { whose: Who, filter: Filter, tgt: u8 },
    /// The event's player exiles cards from the top of their library until
    /// a nonland card (revealed). The controller may put a +1/+1 counter on
    /// the source; if they don't, they may cast that card this turn,
    /// spending mana as though it were mana of any type (CR 609.4b; Black
    /// Widow). Choice: yes/no (`YES` = the counter).
    ExileUntilNonlandCounterOrCast,
    /// Exile all creatures; incubate X, X the number exiled this way (CR
    /// 701.53a: create `token`, an Incubator, with X +1/+1 counters).
    ExileAllCreaturesIncubate { token: u16 },
    /// Each object of `on` gains delayed ability `k` of the source's card as
    /// a triggered ability: "when it dies [or is exiled (`exile`)], ..."
    /// (earthbend; the event's object is the card where it went).
    GrantLeaveTrigger { on: On, k: u8, exile: bool },
    /// If both targets are still legal (and still paired), the controller
    /// of the permanent of requirement `a` simultaneously sacrifices it and
    /// returns the card of requirement `b` (in their graveyard) to the
    /// battlefield (Goblin Welder).
    SwapWithGraveyardCard { a: u8, b: u8 },
    // ---- M7c: the single-card tail, part 2 (ENGINE.md 29) ----
    /// `who` sacrifices n permanents matching `filter` ("you" in the filter
    /// is `who`) of their choice, at once (CR 701.21a): one decision per
    /// permanent in increasing slot order, always leaving enough to reach n;
    /// with n or fewer, all of them without a decision. Choice:
    /// `Sacrifice`.
    SacrificeN { who: Who, filter: Filter, n: u8 },
    /// Put each legal target of requirement `tgt` (a card in a graveyard)
    /// onto the battlefield under the controller's control, `mods` applying
    /// to it as it enters ("it's a Phyrexian in addition to its other
    /// types").
    FromGraveyardAs { tgt: u8, mods: Vec<Mod> },
    /// Return the event's card (in a graveyard or exile) to the battlefield
    /// under its owner's control, `mods` applying to it as it enters ("it's
    /// an enchantment. (It's not a creature.)").
    ReturnEventCardAs { mods: Vec<Mod> },
    /// Harness the source (it stays harnessed while it is that object;
    /// `Mark::Harnessed`).
    Harness,
    /// Exile each legal target of requirement `tgt` (a card in a
    /// graveyard); the controller may play it until the end of their next
    /// turn.
    ExilePlayUntilNextTurn { tgt: u8 },
    /// The controller may exile the event's card from their graveyard,
    /// linked to the source (CR 607.2a). Choice: yes/no (with `optional`).
    ExileEventCardLinked { optional: bool },
    /// The controller puts a card exiled with the source (their choice)
    /// into its owner's graveyard; then creates token `land` if it is a
    /// land card, else token `nonland`. Choice: `ChooseExiled`.
    LinkedToGraveyardThenToken { land: u16, nonland: u16 },
    /// "Until end of turn, you may play lands and cast spells from your
    /// graveyard" (the controller).
    PlayFromGraveyardThisTurn,
    /// "If a card would be put into your graveyard from anywhere this turn,
    /// exile that card instead" (the controller; CR 614.1a).
    GraveyardToExileThisTurn,
    /// Cascade (CR 702.85a), of the event's spell: exile cards from the top
    /// of the controller's library until a nonland card with lesser mana
    /// value; they may cast it without paying its mana cost if its mana
    /// value is less; the rest go on the bottom in a random order. Choice:
    /// yes/no.
    Cascade,
    /// "For each token you control that entered this turn, create a token
    /// that's a copy of it" (the tokens locked in as it resolves, CR 608.2h;
    /// the copies made at once).
    CopyTokensEnteredThisTurn,
    /// Mill n cards; the controller may put a land card from among them into
    /// their hand. Choice: `ChooseGraveyardCard` or `PASS`.
    MillThenLandToHand { n: u8 },
    /// "You may cast a <filter> spell from your graveyard" now (CR 608.2g,
    /// paying its costs); with `exile`, an instant or sorcery spell cast
    /// this way is exiled instead of put into the graveyard. Choice:
    /// `ChooseGraveyardCard` or `PASS`.
    MayCastFromGraveyardNow { filter: Filter, exile: bool },
    /// "After this phase, there is an additional combat phase" (CR 500.8;
    /// during a combat phase).
    ExtraCombat,
    /// Edge of Eternities lane C's instructions (ENGINE.md 99.4).
    EoeC(crate::eoe_c::EoeC),
    /// Warp's delayed ability (CR 702.185a): exile the event's permanent;
    /// its owner may cast it from exile on a later turn.
    WarpExile,
    /// Miracle's trigger (CR 702.94a): the controller may cast the event's
    /// card (revealed, in their hand) by paying its miracle cost. Choice:
    /// yes/no.
    MiracleCast,
    /// Airbend each legal target of requirement `tgt` (a creature or a
    /// spell): exile it; while it is exiled, its owner may cast it for `cost`
    /// rather than its mana cost.
    Airbend { tgt: u8, cost: ManaCost },
    // ---- M7c: the single-card tail, part 3 (`resolve_tail3.rs`, ENGINE.md 30) ----
    /// Each object of `on` gets +p*X/+t*X until end of turn, X the spell's
    /// (locked in as it resolves, CR 608.2h).
    ApplyPtX { on: On, p: i8, t: i8 },
    /// Exile every permanent matching the filter (at once); with `basics`,
    /// for each permanent exiled this way its controller searches their
    /// library for a basic land card, and those players put those cards
    /// onto the battlefield tapped, then shuffle (APNAP; a search may fail,
    /// CR 701.19b). Choice: `LIBRARY` (a definition) or `PASS` = done.
    ExileAll { filter: Filter, basics: Option<bool> },
    /// `who` creates n tokens [tapped].
    TokensFor { who: Who, token: u16, n: u8, tapped: bool },
    /// The controller gains protection from everything until their next
    /// turn (CR 702.16j: a player can't be targeted, damage to them is
    /// prevented).
    PlayerProtection,
    /// Exile the top n cards of `who`'s library (revealed); the controller
    /// may play them until their next end step.
    ExileTopUntilEndStep { who: Who, n: u8, kind: crate::game::PermKind },
    /// The source, a card in a graveyard (still that object), is shuffled
    /// into its owner's library.
    ShuffleSourceIntoLibrary,
    /// If the first legal target of `tgt` is an Equipment, attach it to the
    /// source creature (CR 701.3, 301.5c).
    AttachTargetToSource { tgt: u8 },
    /// Remove a counter of this kind from the source; if it can't, sacrifice
    /// it (fading, CR 702.32a).
    RemoveCounterOrSacrifice { counter: crate::db::Counter },
    /// The cards exiled with the source (CR 607.2a) return to the battlefield
    /// under their owners' control (at once).
    LinkedToBattlefieldOwners,
    /// The source becomes saddled until end of turn (CR 702.171b).
    BecomeSaddled,
    /// Attach the made object (an Equipment) to the event's object, if it is
    /// still a creature on the battlefield (CR 701.3, 301.5c).
    AttachMadeToEventObj,
    /// Put a creature card exiled with the source (CR 607.2a; the
    /// controller's choice if several) onto the battlefield under the
    /// controller's control with a counter of this kind; it becomes the made
    /// object. Choice: `ChooseExiled`.
    LinkedCreatureToBattlefield { counter: crate::db::Counter },
    /// The owner of each legal target of `tgt` (a permanent) puts it into
    /// their library second from the top or on the bottom. Choice: yes/no
    /// (`YES` = second from the top).
    OwnerPutsSecondOrBottom { tgt: u8 },
    /// The event's player gains control of the source and untaps it.
    EventPlayerTakesSourceUntapped,
    /// Exile the top card of the controller's library face down; they look
    /// at it and may play it this turn (CR 406.3).
    ExileTopFaceDownPlayable,
    /// Each player exiles all cards from their hand face down, linked to the
    /// source (CR 607.2a).
    HandsFaceDownLinked,
    /// The face-down cards linked to the source return to their owners'
    /// hands.
    LinkedFaceDownToHand,
    /// If a library has at most n cards, the controller may play the
    /// face-down card linked to the source without paying its mana cost (a
    /// land only if they could play a land now). Choice: yes/no (asked
    /// whenever the card is there and a library is that small).
    PlayLinkedFaceDownFree { library_at_most: u8 },
    /// If the source is on the battlefield: put the creature card it
    /// enchants (`tgt` None) or the first legal target of `tgt` onto the
    /// battlefield under the controller's control and attach the source to
    /// it (`Mark::Reanimated`); with `aura`, the source first becomes an Aura
    /// (that subtype).
    ReanimateAttach { tgt: Option<u8>, aura: Option<u16> },
    /// The creature the source reanimated (`Mark::Reanimated`, as the source
    /// last existed) is sacrificed by its controller.
    SacrificeReanimated,
    /// The Edge of Eternities lane B instructions (ENGINE.md 99.3).
    EoeB(EoeB),
    /// Lane A of the Edge of Eternities port (ENGINE.md 99.2).
    EoeA(EoeA),
    /// The Edge of Eternities cross-lane pass (ENGINE.md 99.8).
    EoeX(crate::eoe_x::EoeX),
    /// Edge of Eternities workstream W3 (ENGINE.md 99.13).
    EoeW3(crate::eoe_w3::W3),
    /// W4 of the Edge of Eternities port (ENGINE.md 99.14).
    EoeW4(crate::eoe_w4::EoeW4),
    /// Edge of Eternities W1, the IR-fixed cards (ENGINE.md 99.11).
    EoeW1(crate::eoe_w1::EoeW1),
    /// Edge of Eternities W2, mana and lands (ENGINE.md 99.12).
    EoeW2(crate::eoe_w2::EoeW2),
}

/// What a delayed trigger is about ("that card", "that much").
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum About {
    Nothing,
    /// The object the last `CreateToken` or put-onto-the-battlefield made.
    Made,
    /// The number remembered (`Amount::LkiManaValue`).
    Remembered,
    /// Each object the last `CreateToken` (or token copy) made: one delayed
    /// ability about each.
    EachMade,
}

/// Whose copiable values a copy takes (CR 707.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CopyOf {
    /// The first legal target of requirement `i`: a permanent (its copiable
    /// values now), or a card in a graveyard (its card's); with `lki`, a
    /// permanent that left the battlefield during this resolution as it last
    /// existed there (CR 608.2h, 111.12).
    Target { i: u8, lki: bool },
    /// The ability's source, as it last existed on the battlefield if it
    /// left.
    Source,
    /// The permanent the source is attached to (an Aura's enchanted
    /// creature).
    SourceHost,
    /// The ability's source card as it last existed in a graveyard (its
    /// printed values: eternalize, "exile this card from your graveyard:
    /// create a token that's a copy of it").
    SourceCard,
}

/// Which spell a copy is a copy of (CR 707.10).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpellOf {
    /// The resolving spell itself ("copy this spell" in its own text).
    This,
    /// The triggered ability's event spell ("when you cast this spell,
    /// copy it"; as it last existed on the stack if it left, CR 608.2h).
    EventSpell,
    /// The first legal target spell of requirement `i`.
    Target(u8),
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
    /// Exiled face down, linked to the source (hideaway, CR 702.75a).
    FaceDown,
    /// On top of the library, in the order chosen (the last on top).
    Top,
    /// On the bottom, in the order chosen (the last at the very bottom).
    Bottom,
    Graveyard,
    Hand,
    /// Onto the battlefield tapped under the controller's control
    /// (Famished Worldsire; ENGINE.md 99.12).
    BattlefieldTapped,
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
        let amt = |a: &Amount| matches!(a, Amount::EventAmount | Amount::EventObjPower | Amount::EventTimes | Amount::EventSpellTimes | Amount::EventSpellMv);
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
            Op::Ext(Ext::ReturnEventCardAs { .. }) | Op::Ext(Ext::ExileEventCardLinked { .. }) => true,
            Op::Ext(Ext::SacrificeN { who: w, .. }) => who(w),
            Op::Ext(Ext::Cascade) | Op::Ext(Ext::WarpExile) | Op::Ext(Ext::MiracleCast) => true,
            Op::Ext(Ext::AddColorless { n }) => amt(n),
            Op::Ext(Ext::DamageFrom { event_obj, amount, .. }) => *event_obj || amt(amount),
            Op::Ext(Ext::Reflexive { .. })
            | Op::Ext(Ext::ReturnEventObjToHand)
            | Op::Ext(Ext::MayMillThenTakeCreature)
            | Op::Ext(Ext::ExileEventObj2) => true,
            Op::Ext(Ext::ReturnDiedWithCounter { .. }) => true,
            Op::Ext(Ext::JumpUnlessEventObj { .. }) | Op::Ext(Ext::GainControlOfEventSpellTargets { .. }) => true,
            Op::Ext(Ext::CreateTokenCopy { n, who: w, .. }) => who(w) || amt(n),
            Op::Ext(Ext::CopySpell { of, n, who: w, .. }) => *of == SpellOf::EventSpell || who(w) || amt(n),
            Op::Ext(Ext::ExileEventCard) => true,
            Op::Ext(Ext::CounterEventItem) => true,
            Op::Ext(Ext::EoeC(crate::eoe_c::EoeC::ReturnEventCardTapped)) => true,
            Op::Ext(Ext::EventControllerBecomesMonarch) => true,
            Op::Ext(Ext::OfferPayOther { who: w, .. }) => who(w),
            Op::Ext(Ext::ExileTopWithPermission { who: w, .. }) => who(w),
            Op::Ext(Ext::Poison { who: w, n }) => who(w) || amt(n),
            Op::Ext(Ext::EoeA(EoeA::DestroyEventObj)) => true,
            Op::Ext(Ext::ApplyPtPerCount { on, .. })
            | Op::Ext(Ext::TapOrUntap { on })
            | Op::Ext(Ext::CounterOrDoubleOnSecond { on }) => matches!(on, On::EventObj),
            Op::Ext(Ext::CreateTokensFor { who: w, .. }) | Op::Ext(Ext::GraveyardToBottomRandom { who: w }) => who(w),
            Op::Ext(Ext::EoeD(x)) => x.uses_event(),
            Op::Ext(Ext::EoeX(x)) => x.uses_event(),
            Op::Ext(Ext::EoeW3(x)) => x.uses_event(),
            Op::Ext(Ext::EoeW4(x)) => x.uses_event(),
            Op::Ext(Ext::EoeW1(x)) => x.uses_event(),
            Op::Ext(Ext::EoeW2(x)) => x.uses_event(),
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
            Op::Ext(Ext::OfferPayOther { if_paid, .. }) => Some(if_paid),
            Op::Ext(Ext::EoeB(x)) => x.jump_target(),
            Op::Ext(Ext::EoeX(x)) => x.jump_target(),
            Op::Ext(Ext::EoeW3(x)) => x.jump_target(),
            Op::Ext(Ext::EoeW4(x)) => x.jump_target(),
            Op::Ext(Ext::EoeW1(x)) => x.jump_target(),
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
        let req = TargetReq { filter, min, max, distinct: 0, pair: 0, total_mv: None };
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

    /// Add a target requirement whose targets must differ from those of
    /// the requirements in `distinct` (a bit mask; "another target", CR
    /// 115.3); returns its index.
    pub fn target_distinct(&mut self, filter: Filter, min: u8, max: u8, distinct: u8) -> u8 {
        let t = self.target(filter, min, max);
        self.targets[t as usize].distinct = distinct;
        t
    }

    /// Insert `op` at position `at` (before what is there): jumps past `at`
    /// move with the ops they jump to.
    pub fn insert(&mut self, at: usize, op: Op) {
        for o in self.program.iter_mut() {
            if let Some(t) = o.jump_target() {
                if *t as usize > at {
                    *t += 1;
                }
            }
        }
        self.program.insert(at, op);
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

/// The instructions of lane A of the Edge of Eternities port (ENGINE.md
/// 99.2), inside `Ext::EoeA` (`resolve_eoe_a.rs`). Same rules as `Op`: named
/// after rules actions, never cards.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeA {
    /// The owner of the first legal target of requirement `tgt` (a
    /// permanent) puts it on top of or on the bottom of their library, their
    /// choice. Choice (the owner's): yes/no (`YES` = top).
    OwnerPutsTopOrBottom { tgt: u8 },
    /// "Destroy it": destroy the object of the triggered ability's event, if
    /// it is still that permanent (CR 701.8a; "when enchanted creature
    /// becomes tapped or is dealt damage, destroy it").
    DestroyEventObj,
    /// Remember how many cards the looked-at pile holds (`Frame::x`), for
    /// `CounterOnSourceUnlessPileShrank`.
    MarkPile,
    /// "If you didn't put a card into your hand this way, put a <counter>
    /// on this": unless the pile holds fewer cards than `MarkPile` saw, put
    /// a counter of this kind on the source (if it is still that permanent).
    CounterOnSourceUnlessPileShrank { counter: Counter },
    /// "Put its counters on target creature you control": the source's
    /// counters (now, or as it last existed on the battlefield, CR 608.2h),
    /// every kind and number, on the first legal target of `tgt` (CR 122.6).
    PutSourceCountersOnTarget { tgt: u8 },
}
