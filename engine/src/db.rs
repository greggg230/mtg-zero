//! Lowered card definitions and the card database (ENGINE.md 3).

use crate::ids::DefId;
use crate::lower;
use crate::mana::{ManaCost, ManaMask};
use crate::program::Op;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Card type bits (CR 300.1).
pub mod types {
    pub const ARTIFACT: u16 = 1 << 0;
    pub const CREATURE: u16 = 1 << 1;
    pub const ENCHANTMENT: u16 = 1 << 2;
    pub const INSTANT: u16 = 1 << 3;
    pub const LAND: u16 = 1 << 4;
    pub const PLANESWALKER: u16 = 1 << 5;
    pub const SORCERY: u16 = 1 << 6;
    pub const BATTLE: u16 = 1 << 7;
    pub const KINDRED: u16 = 1 << 8;
    /// Permanent types (CR 110.4).
    pub const PERMANENT: u16 = ARTIFACT | CREATURE | ENCHANTMENT | LAND | PLANESWALKER | BATTLE;

    pub fn from_name(s: &str) -> Option<u16> {
        Some(match s {
            "Artifact" => ARTIFACT,
            "Creature" => CREATURE,
            "Enchantment" => ENCHANTMENT,
            "Instant" => INSTANT,
            "Land" => LAND,
            "Planeswalker" => PLANESWALKER,
            "Sorcery" => SORCERY,
            "Battle" => BATTLE,
            "Kindred" | "Tribal" => KINDRED,
            _ => return None,
        })
    }
}

/// Supertype bits (CR 205.4).
pub mod supertypes {
    pub const BASIC: u8 = 1 << 0;
    pub const LEGENDARY: u8 = 1 << 1;
    pub const SNOW: u8 = 1 << 2;
    pub const WORLD: u8 = 1 << 3;

    pub fn from_name(s: &str) -> Option<u8> {
        Some(match s {
            "Basic" => BASIC,
            "Legendary" => LEGENDARY,
            "Snow" => SNOW,
            "World" => WORLD,
            _ => return None,
        })
    }
}

/// Keyword abilities the engine implements (CR 702).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Kw {
    Flying,
    Reach,
    FirstStrike,
    DoubleStrike,
    Deathtouch,
    Trample,
    Lifelink,
    Vigilance,
    Haste,
    Defender,
    Menace,
    Flash,
    Hexproof,
    Shroud,
    Indestructible,
}

impl Kw {
    /// Keywords a keyword counter can grant (CR 122.1b).
    pub fn from_counter_name(s: &str) -> Option<Kw> {
        Some(match s {
            "flying" => Kw::Flying,
            "first strike" => Kw::FirstStrike,
            "double strike" => Kw::DoubleStrike,
            "deathtouch" => Kw::Deathtouch,
            "hexproof" => Kw::Hexproof,
            "indestructible" => Kw::Indestructible,
            "lifelink" => Kw::Lifelink,
            "menace" => Kw::Menace,
            "reach" => Kw::Reach,
            "trample" => Kw::Trample,
            "vigilance" => Kw::Vigilance,
            "haste" => Kw::Haste,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct KwSet(pub u32);

impl KwSet {
    #[inline]
    pub fn has(self, k: Kw) -> bool {
        self.0 & (1 << k as u32) != 0
    }
    pub fn insert(&mut self, k: Kw) {
        self.0 |= 1 << k as u32;
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// What a protection ability protects from (CR 702.16a): a quality of the
/// other object, or a player.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct Prot {
    /// Colours (W U B R G = bits 0..4).
    pub colors: ManaMask,
    pub colorless: bool,
    /// Protection from everything (702.16k).
    pub everything: bool,
    /// Card types of the other object (`types::*`): creatures, instants.
    pub types: u16,
    /// From the protected permanent's controller's opponents (702.16j:
    /// "from each of your opponents"; evaluated against its controller).
    pub opponents: bool,
    /// From the player chosen as the permanent entered (True-Name Nemesis).
    pub chosen_player: bool,
}

impl Prot {
    pub fn is_empty(&self) -> bool {
        *self == Prot::default()
    }
    pub fn union(&mut self, o: &Prot) {
        self.colors |= o.colors;
        self.colorless |= o.colorless;
        self.everything |= o.everything;
        self.types |= o.types;
        self.opponents |= o.opponents;
        self.chosen_player |= o.chosen_player;
    }
}

/// What a continuous effect or static ability does to an object (CR 613).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Mod {
    /// Layer 4: gains card types; gains subtypes (with `replace`, its
    /// creature types become exactly these, CR 205.1b).
    Types { add: u16, subtypes: Vec<u16>, replace: bool },
    /// Layer 5: its colours become exactly these.
    Colors(ManaMask),
    /// Layer 6: gains keywords.
    Keywords(KwSet),
    /// Layer 6: gains protection.
    Protection(Prot),
    /// Layer 7b: its base power and toughness become p/t.
    SetPt(i32, i32),
    /// Layer 7c: +p/+t.
    Pt(i32, i32),
    /// Rules effects (not characteristics): it can't block.
    CantBlock,
    /// It can't be blocked.
    CantBeBlocked,
    /// It attacks each combat if able (CR 508.1d).
    MustAttack,
    /// "If it would die this turn, exile it instead" (CR 614).
    ExileIfDies,
    /// "Whenever it attacks the monarch this turn, it gains these keywords
    /// until end of turn."
    IfAttacksMonarch(KwSet),
    /// Layer 7c: +p/+t for each permanent matching the filter (relative to
    /// the static ability's controller; a static ability only).
    PtPer { filter: Filter, p: i32, t: i32 },
    /// Layer 6: it loses all abilities (CR 613.1f).
    LoseAllAbilities,
    /// Layer 4: its card types become exactly `types` and its creature
    /// types exactly `subtypes` (CR 205.1a: "becomes a [subtype] creature";
    /// supertypes are kept).
    BecomeTypes { types: u16, subtypes: Vec<u16> },
    /// Layer 6: it gains latent activated ability k of its card ("this
    /// Saga gains '<ability>'"; `CardDef::latent`).
    GainLatent(u8),
}

/// Which objects a static ability affects (CR 611.3a: re-evaluated
/// continuously).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Scope {
    /// The permanent itself.
    This,
    /// The creature it equips (CR 301.5).
    Equipped,
    /// Permanents matching the filter, relative to the source (its
    /// controller is "you"); `other`: not the source itself. The filter
    /// uses types, subtypes, controller and token-ness only (never power
    /// or toughness, which statics change).
    Matching { filter: Filter, other: bool },
}

/// A static ability that modifies objects (CR 604, 611.3), while its
/// condition holds for its controller.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct StaticDef {
    pub scope: Scope,
    pub cond: Option<Cond>,
    pub mods: Vec<Mod>,
}

/// A counter kind (CR 122).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Counter {
    /// +1/+1 (122.1a).
    P1P1,
    /// -1/-1.
    M1M1,
    /// A keyword counter (122.1b).
    Kw(Kw),
    /// A stun counter (122.1d): untapping removes one instead.
    Stun,
    /// Any other named counter (interned name: level, story, ...).
    Named(u16),
    /// A loyalty counter (CR 122.1e, 306.5c): a planeswalker's loyalty.
    Loyalty,
    /// A lore counter (CR 714.3): a Saga's progress.
    Lore,
}

/// A characteristic-defining power / toughness (CR 604.3, layer 7a).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum PtCda {
    /// Power = card types among cards in all graveyards; toughness = that
    /// plus `plus` (the Lhurgoyfs).
    GraveyardTypes { plus: i32 },
    /// Power = the permanents you control matching the filter (a static
    /// scope filter: types, subtypes, controller); toughness printed.
    PowerCount(Filter),
    /// Power and toughness = basic land types among lands you control
    /// (domain).
    Domain,
    /// Level up (CR 711): base power and toughness by the number of
    /// counters of this kind, per range (lo, hi or none, p, t).
    Leveler { counter: Counter, ranges: Vec<(u8, Option<u8>, i32, i32)> },
}

/// A predicate over targets (players and objects). At top level the type
/// predicates mean *permanents* (on the battlefield) of that type, `Player`
/// means a player, and `Spell(f)` means a spell on the stack whose card
/// matches `f` by its characteristics. The other predicates test an
/// object's characteristics (and, for `Controller`, who controls it,
/// relative to the ability's controller).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Filter {
    Player,
    /// Has any of these card types (`types::*` bits).
    Type(u16),
    Spell(Box<Filter>),
    /// Total power plus toughness at most n (a creature).
    TotalPtAtMost(i32),
    /// Has this subtype (interned id).
    Subtype(u16),
    /// Has any of these supertypes (`supertypes::*` bits).
    Supertype(u8),
    /// Mana value at most n (CR 202.3).
    ManaValueAtMost(u8),
    /// A creature with power at least / at most n, toughness at most n.
    PowerAtLeast(i32),
    PowerAtMost(i32),
    ToughnessAtMost(i32),
    /// Controlled by the ability's controller (`true`) or an opponent.
    Controller(bool),
    /// The spell or ability's source itself ("another": `Not(Source)`).
    Source,
    /// Has any of these colours.
    Color(ManaMask),
    /// A player who is an opponent of the controller.
    Opponent,
    And(Box<Filter>, Box<Filter>),
    Or(Vec<Filter>),
    Not(Box<Filter>),
    /// Matches anything (a spell with no restriction: `Spell(Any)`).
    Any,
    /// A token (CR 111).
    Token,
    /// An attacking creature (CR 506.4).
    Attacking,
    /// A tapped permanent.
    Tapped,
    /// A player who has drawn exactly n cards this turn.
    DrewThisTurn(u8),
    /// A permanent whose evoke cost was paid (CR 702.74a).
    Evoked,
    /// A card in a graveyard matching the inner filter (a target).
    InGraveyard(Box<Filter>),
    /// Owned by the ability's controller (`true`) or an opponent.
    Owner(bool),
    /// A player drawing the first card of their own draw step (as the
    /// draw happens).
    FirstDrawInDrawStep,
    /// A permanent with no counters of this kind.
    NoCounters(Counter),
    /// A player who is the active player's opponent and controls more
    /// creatures than the active player (Oath of Druids).
    OathTarget,
    /// An unblocked attacking creature (CR 509.1h; ninjutsu, 702.49a):
    /// attacking, blockers declared, not blocked.
    Unblocked,
    /// Mana value at least n (CR 202.3).
    ManaValueAtLeast(u8),
    /// Has this keyword.
    HasKeyword(Kw),
    /// Colorless (CR 105.2c).
    Colorless,
    /// A spell with a target matching the inner filter (one or more of its
    /// targets, as chosen).
    TargetsMatching(Box<Filter>),
    /// An object with one or more counters of this kind (in any zone).
    HasCounter(Counter),
    /// Has exactly one of these mana costs (CR 202.1; a land has no mana
    /// cost, so not {0}; nor has a card with {X} in its cost a cost without).
    ManaCostIn(Vec<ManaCost>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TargetReq {
    pub filter: Filter,
    pub min: u8,
    pub max: u8,
}

/// A spell or ability's effect: its target requirements and program; or,
/// for a modal one (CR 700.2), its modes (`targets` and `program` empty).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct AbilityDef {
    pub targets: Vec<TargetReq>,
    pub program: Vec<Op>,
    pub modes: Option<Box<Modes>>,
    /// "n damage divided as you choose among" the targets of requirement
    /// `.0`: the division is announced with the targets (CR 601.2d).
    pub divide: Option<(u8, DivAmount)>,
}

/// The amount of divided damage (CR 601.2d).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DivAmount {
    Fixed(u8),
    /// The spell's X, twice X if X is at least `double_at` (0: never).
    X { double_at: u8 },
}

impl DivAmount {
    /// The amount for a spell cast with this X.
    pub fn of(self, x: u8) -> u8 {
        match self {
            DivAmount::Fixed(n) => n,
            DivAmount::X { double_at } if double_at > 0 && x >= double_at => x.saturating_mul(2),
            DivAmount::X { .. } => x,
        }
    }
}

/// The modes of a modal spell or ability (CR 700.2): choose between `min`
/// and `max` of them, the same one more than once if `repeat`. Each mode is
/// an ability of its own (its targets and program).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Modes {
    pub list: Vec<AbilityDef>,
    pub min: u8,
    pub max: u8,
    pub repeat: bool,
    /// Escalate: discard a card for each mode beyond the first (CR
    /// 702.120).
    pub escalate_discard: bool,
}

impl AbilityDef {
    /// Does any op (of any mode) read a triggered ability's event?
    pub fn uses_event(&self) -> bool {
        self.program.iter().any(|op| op.uses_event())
            || self.modes.as_ref().is_some_and(|m| m.list.iter().any(|x| x.uses_event()))
    }

    /// Does any part of it target (any mode, for a modal one)?
    pub fn has_targets(&self) -> bool {
        !self.targets.is_empty() || self.modes.as_ref().is_some_and(|m| m.list.iter().any(|x| x.has_targets()))
    }

    /// The target requirements of the chosen mode instances in order (the
    /// ability's own for a non-modal one).
    pub fn reqs(&self, modes: &[u8]) -> Vec<&TargetReq> {
        match &self.modes {
            None => self.targets.iter().collect(),
            Some(m) => modes.iter().flat_map(|&k| m.list[k as usize].targets.iter()).collect(),
        }
    }
}

/// Events triggered abilities listen for (ENGINE.md 8). Each kind has one
/// or two subjects (`Slot`): the event's object or player, as noted.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum EventKind {
    /// A permanent entered the battlefield (CR 603.6a). Subject: obj.
    Enters,
    /// A permanent was put into a graveyard from the battlefield ("dies"
    /// for a creature, CR 700.4). Subject: obj (last known information).
    Dies,
    /// A permanent left the battlefield (603.6c). Subject: obj.
    LeavesBattlefield,
    /// A creature was declared as an attacker (508.1m). Subject: attacker.
    Attacks,
    /// The active player declared one or more attackers. Subject: player.
    YouAttack,
    /// A creature dealt combat damage to a player or a planeswalker
    /// (510.3a). Subjects: source, target (the player or planeswalker);
    /// with the amount.
    DealsCombatDamage,
    /// A spell was cast (601.2i). Subjects: spell, controller.
    IsCast,
    /// A player drew a card (121.2). Subject: player.
    DrawsCard,
    /// A permanent became tapped. Subject: obj.
    BecomesTapped,
    /// A permanent was sacrificed (701.17). Subject: obj.
    IsSacrificed,
    /// The beginning of a step or phase (503.1a, 507.1, ...). Subject: the
    /// active player.
    BeginningOfUpkeep,
    BeginningOfDrawStep,
    BeginningOfPrecombatMain,
    BeginningOfCombat,
    BeginningOfEndStep,
    /// A permanent was dealt damage. Subject: obj; with the amount.
    DealtDamage,
    /// A creature attacks alone (exalted, CR 702.83a). Subject: attacker.
    AttacksAlone,
    /// A player discarded a card (701.9). Subjects: player, card.
    CardDiscarded,
    /// A permanent became monstrous (701.37b). Subject: obj.
    BecomesMonstrous,
    /// A player created one or more creature tokens (one event per
    /// effect). Subject: player.
    CreatureTokensCreated,
    /// A creature dealt combat damage to a creature (510.2). Subjects:
    /// source, target.
    DealsCombatDamageToCreature,
    /// A creature was declared as a blocker (509.1). Subjects: blocker,
    /// attacker.
    Blocks,
    /// Never emitted: the trigger is a delayed triggered ability (CR 603.7)
    /// an effect creates (`Ext::Delay`).
    Delayed,
    /// A permanent transformed (CR 701.27; "when this transforms into",
    /// 701.27e: seen with the face it transformed into). Subject: obj.
    Transforms,
    /// Lore counters put on a Saga reached chapter `amount` (CR 714.2b: the
    /// number of lore counters was less than N and became at least N; one
    /// event per chapter number reached). Subject: obj.
    Chapter,
    /// A source dealt damage, combat or not (CR 120; "whenever this deals
    /// damage"); one event per source and damage event, with the total
    /// (CR 120.? simultaneous damage). Subject: source.
    DealsDamage,
}

/// When a delayed triggered ability triggers (CR 603.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DelayedWhen {
    /// At the beginning of the next end step.
    NextEndStep,
    /// At the beginning of the next turn's upkeep.
    NextUpkeep,
    /// At the beginning of its controller's next main phase.
    YourNextMainPhase,
}

impl EventKind {
    /// The event's subject for a slot (`Default`: its only subject).
    /// `Some(0)` = the first subject, `Some(1)` = the second.
    pub fn slot_index(self, s: Slot) -> Option<u8> {
        use EventKind as E;
        let names: (&[Slot], &[Slot]) = match self {
            E::Enters
            | E::Dies
            | E::LeavesBattlefield
            | E::BecomesTapped
            | E::IsSacrificed
            | E::DealtDamage
            | E::BecomesMonstrous => {
                (&[Slot::Obj], &[])
            }
            E::Attacks | E::AttacksAlone => (&[Slot::Attacker], &[]),
            E::Blocks => (&[Slot::Blocker], &[]),
            E::Transforms | E::Chapter => (&[Slot::Obj], &[]),
            E::DealsDamage => (&[Slot::Source], &[]),
            E::YouAttack
            | E::DrawsCard
            | E::BeginningOfUpkeep
            | E::BeginningOfDrawStep
            | E::BeginningOfPrecombatMain
            | E::BeginningOfCombat
            | E::BeginningOfEndStep
            | E::CreatureTokensCreated => (&[Slot::Player], &[]),
            E::DealsCombatDamage | E::DealsCombatDamageToCreature => (&[Slot::Source], &[Slot::Target]),
            E::CardDiscarded => (&[Slot::Player], &[Slot::Card]),
            E::Delayed => (&[], &[]),
            E::IsCast => (&[Slot::Spell], &[Slot::Controller]),
        };
        match s {
            Slot::Default if names.1.is_empty() => Some(0),
            Slot::Default => None,
            s if names.0.contains(&s) => Some(0),
            s if names.1.contains(&s) => Some(1),
            _ => None,
        }
    }
}

/// A named subject of an event (the IR's `EventSubject`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Slot {
    /// The event's only subject.
    Default,
    Obj,
    Attacker,
    Blocker,
    Source,
    Target,
    Spell,
    Controller,
    Player,
    Card,
}

/// When a triggered ability triggers (CR 603.2), over the event's subjects
/// and the ability's source.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TrigPred {
    Always,
    And(Box<TrigPred>, Box<TrigPred>),
    Or(Box<TrigPred>, Box<TrigPred>),
    Not(Box<TrigPred>),
    /// The subject is the ability's source ("this creature").
    IsSelf(Slot),
    /// The subject (an object, or a player) matches the filter.
    Subject(Slot, Filter),
    /// The subject (an object: its controller; a player: itself) is the
    /// ability's controller (`true`) or an opponent.
    Controlled(Slot, bool),
    /// The subject is the permanent this source (an Equipment) was
    /// attached to ("whenever equipped creature dies"; seen as it was).
    FormerHost(Slot),
    /// "Whenever equipped creature and at least one other creature
    /// attack": the source's host attacks, with another attacker.
    HostAttacksWithAnother,
    /// The subject is the permanent this source (an Equipment) is attached
    /// to now (a triggered ability it grants, CR 301.5c-like).
    IsHost(Slot),
    /// An intervening "if" (CR 603.4): checked when the event happens and
    /// again on resolution.
    If(Cond),
    /// The ability's source matches the filter when the event happens
    /// ("if it was evoked").
    Source(Filter),
    /// A condition of the trigger event itself, checked when it happens
    /// only ("whenever you cast your second spell each turn").
    State(Cond),
    /// A chapter ability (CR 714.2b): this Saga's lore counters reached
    /// chapter n (a `Chapter` event of the source with amount n).
    Chapter(u8),
    /// "Whenever one or more ...": triggers once for the events of one
    /// simultaneous move (a batch, `Game::begin_batch`), however many match.
    Batch,
}

impl TrigPred {
    /// The intervening "if" clauses at the top level (re-checked on
    /// resolution, CR 603.4).
    pub fn intervening(&self) -> Vec<&Cond> {
        match self {
            TrigPred::If(c) => vec![c],
            TrigPred::And(a, b) => {
                let mut v = a.intervening();
                v.extend(b.intervening());
                v
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TriggerDef {
    pub event: EventKind,
    pub pred: TrigPred,
    pub ability: AbilityDef,
    /// Its effect reads the event ("that creature", "that player", "that
    /// much"): two instances with different events are not
    /// interchangeable.
    pub uses_event: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbilitySlot {
    Spell,
    Trigger(u8),
    /// `CardDef.activated[i]` (abilities of a permanent).
    Activated(u8),
    /// `CardDef.hand_abilities[i]` (activated from hand: cycling).
    Hand(u8),
}

/// A condition, evaluated for a player (the ability's controller): "if",
/// "activate only if", "enters tapped unless".
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Cond {
    /// You control at least n permanents matching the filter.
    ControlAtLeast(Filter, u8),
    /// You control at most n permanents matching the filter.
    ControlAtMost(Filter, u8),
    /// You have exactly n cards in hand.
    HandExactly(u8),
    /// At least n cards in your graveyard (threshold).
    GraveyardAtLeast(u8),
    /// At least n card types among cards in your graveyard (delirium).
    Delirium(u8),
    /// A permanent you controlled left the battlefield this turn (revolt).
    Revolt,
    /// It is your turn and one of your first n turns of the game.
    YourTurnAtMost(u8),
    /// You have at most n cards in hand.
    HandAtMost(u8),
    /// Your devotion to the colour (index W U B R G) is at least the
    /// number of cards in your library (Thassa's Oracle).
    DevotionCoversLibrary(u8),
    /// You have cast no spell this game.
    FirstSpellThisGame,
    /// You are the monarch (CR 724).
    IsMonarch,
    /// A static ability's source has between lo and hi (none: no upper
    /// bound) counters of this kind (level up).
    SourceCountersIn(Counter, u8, Option<u8>),
    /// You gained life this turn.
    GainedLifeThisTurn,
    /// An opponent has more cards in hand than you.
    OpponentHasMoreCards,
    /// You have cast exactly n spells this turn (noncreature ones only if
    /// `noncreature`).
    SpellsCastThisTurn { n: u8, noncreature: bool },
    /// Your library has no cards in it.
    LibraryEmpty,
    /// It is your turn.
    YourTurn,
    /// Your life total is at most half your starting life total.
    LifeAtMostHalfStart,
    /// You control a permanent of one of these colours other than the
    /// ability's source (evaluated with the source: `cond_holds_from`).
    ControlOtherOfColor(crate::mana::ManaMask),
    /// All of these hold (a static ability's own conditions included).
    All(Vec<Cond>),
}

/// What a mana ability makes (CR 106.1, 605.1a).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ManaProd {
    /// One mana of one type in the mask (a basic land; Birds of Paradise).
    One(ManaMask),
    /// Fixed amounts per type (Sol Ring: `{C}{C}`).
    Fixed([u8; 6]),
    /// n mana of one type chosen from the mask (Black Lotus).
    SameType { mask: ManaMask, n: u8 },
    /// n mana in any combination of types in the mask (Relic of Sauron).
    Combo { mask: ManaMask, n: u8 },
    /// One mana of type `ty` per permanent you control matching the filter
    /// (Gaea's Cradle).
    PerPermanent { ty: u8, filter: Filter },
    /// One mana of any colour among permanents you control matching the
    /// filter (Mox Amber).
    ColorAmong(Filter),
}

/// A mana ability's effect: the mana, plus damage its source deals to its
/// controller (Talismans, Ancient Tomb).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ManaAbility {
    pub prod: ManaProd,
    pub damage_to_you: u8,
}

/// Sacrifice n permanents you control matching the filter (`another`: not
/// the ability's source).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SacCost {
    pub filter: Filter,
    pub n: u8,
    pub another: bool,
}

/// A cost (CR 118, 602.2b). The mana part is paid first (601.2g-h: a
/// permanent tapped for mana can then be sacrificed), then the rest
/// (ENGINE.md 18).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Cost {
    pub mana: ManaCost,
    /// `{T}` (CR 107.5): the source must be untapped and, if it is a
    /// creature, free of summoning sickness (302.6).
    pub tap: bool,
    /// Pay n life (CR 119.4).
    pub life: u8,
    pub sac_self: bool,
    /// Exile this card (a Spirit Guide from hand).
    pub exile_self: bool,
    /// Discard this card (cycling, CR 702.29a).
    pub discard_self: bool,
    pub discard_hand: bool,
    pub sacrifice: Option<SacCost>,
    /// Discard n cards of your choice (matching `discard_filter`, if any:
    /// retrace's "a land card").
    pub discard: u8,
    pub discard_filter: Option<Filter>,
    /// Exile a card matching the filter from your hand (not this card).
    pub exile_from_hand: Option<Filter>,
    /// Return n permanents you control matching the filter to their
    /// owner's hand.
    pub bounce: Option<SacCost>,
    /// Exile n cards from your graveyard (chosen as the cost is paid).
    pub exile_graveyard: u8,
    /// Remove n counters of a kind from the source (CR 118.3).
    pub remove_counters: Option<(Counter, u8)>,
    /// Crew n (CR 702.122): tap untapped creatures you control (not the
    /// source) with total power n or more.
    pub crew: u8,
}

/// An alternative cost (CR 118.9): paid instead of the mana cost.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct AltCost {
    /// Its mana (zero: none).
    pub mana: ManaCost,
    /// The rest: life, a card exiled from hand, permanents returned or
    /// sacrificed.
    pub cost: Cost,
    /// Only during your turn (`Some(true)`) or not (`Some(false)`).
    pub turn: Option<bool>,
    /// Only if this holds for you ("if you control a Swamp").
    pub only_if: Option<Cond>,
    /// Evoke (CR 702.74): the permanent knows it was evoked.
    pub evoke: bool,
    /// Dash (CR 702.109): the permanent gains haste and delayed ability
    /// `k` (`delayed_base + k`) returns it to its owner's hand at the
    /// beginning of the next end step.
    pub dash: Option<u8>,
}

/// What an activated ability does.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ActEffect {
    /// A mana ability (CR 605): resolves at once, never on the stack.
    Mana(ManaAbility),
    /// Goes on the stack (CR 602.2a).
    Stack(AbilityDef),
}

/// An activated ability (CR 602) of a permanent or of a card in hand.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ActivatedDef {
    pub cost: Cost,
    pub effect: ActEffect,
    /// "Activate only as a sorcery" (CR 602.5d).
    pub sorcery: bool,
    /// "Activate only as an instant." For a mana ability: only with
    /// priority, never while paying a cost (Lion's Eye Diamond).
    pub instant_only: bool,
    /// "Activate only if ..." (CR 602.5b).
    pub only_if: Option<Cond>,
    /// "Activate only during your turn."
    pub your_turn: bool,
    /// A loyalty ability (CR 606.2): the change in loyalty its cost makes
    /// (+1 = put a loyalty counter, -3 = remove three; 606.4).
    pub loyalty: Option<i8>,
}

impl ActivatedDef {
    pub fn mana(&self) -> Option<&ManaAbility> {
        match &self.effect {
            ActEffect::Mana(m) => Some(m),
            ActEffect::Stack(_) => None,
        }
    }
}

/// How a permanent enters the battlefield (CR 614.1c, 614.12).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum EntersTapped {
    #[default]
    No,
    Always,
    /// Tapped unless the condition holds for its controller as it enters.
    Unless(Cond),
}

/// Names one ability of one card definition; `Copy`, so stack items and
/// frames carry it instead of the program.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AbilityRef {
    pub def: DefId,
    pub slot: AbilitySlot,
    /// One mode of a modal ability (a resolving mode instance).
    pub mode: Option<u8>,
}

/// A lowered card (ENGINE.md 3). Read only through `Game` accessors during
/// play, so layers can later modify the values.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CardDef {
    pub name: String,
    pub types: u16,
    pub supertypes: u8,
    pub subtypes: Vec<u16>,
    pub colors: ManaMask,
    pub cost: ManaCost,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub keywords: KwSet,
    /// The spell ability of an instant or sorcery.
    pub spell: Option<AbilityDef>,
    pub triggers: Vec<TriggerDef>,
    /// Activated abilities of the permanent, mana abilities included (the
    /// intrinsic "{T}: Add" of basic land types and the IR's `produces`
    /// comes first). At most 4 (the `ACTIVATE` id range).
    pub activated: Vec<ActivatedDef>,
    /// Abilities activated from hand (cycling, Spirit Guides). At most 1.
    pub hand_abilities: Vec<ActivatedDef>,
    pub enters_tapped: EntersTapped,
    /// "As this enters, you may pay n life. If you don't, it enters
    /// tapped." (shock lands; 0 = none).
    pub shock_life: u8,
    /// "This spell can't be countered." (CR 101.2)
    pub uncounterable: bool,
    /// Additional costs to cast it: the caster pays exactly one of these
    /// (none when empty; their mana parts are unused).
    pub additional: Vec<Cost>,
    /// A token's definition (CR 111): created by effects, never in a deck.
    pub token: bool,
    /// Static abilities that modify objects (CR 604, 613).
    pub statics: Vec<StaticDef>,
    /// A characteristic-defining power / toughness (layer 7a).
    pub pt_cda: Option<PtCda>,
    /// "Can't be blocked except by n or more creatures" (CR 509.1b; 0 = no
    /// such ability; menace is the keyword).
    pub min_blockers: u8,
    /// "Can block only creatures with flying."
    pub blocks_only_flyers: bool,
    /// "Can't be blocked by" creatures matching this filter.
    pub unblockable_by: Option<Filter>,
    /// "Doesn't untap during your untap step" (CR 502.3).
    pub no_untap: bool,
    /// Counters it enters the battlefield with (CR 122.6).
    pub enters_counters: Vec<(Counter, u8)>,
    /// Bit k set: one of its triggered abilities listens for `EventKind` k.
    pub trig_mask: u32,
    /// Printed protection (CR 702.16).
    pub protection: Prot,
    /// "Spells matching the filter cost {n} more to cast" (every player's;
    /// CR 601.2f).
    pub spell_tax: Vec<(Filter, u8)>,
    /// "Activated abilities of [these types] can't be activated" (card
    /// type bits; mana abilities included, CR 602.5).
    pub locks_activated: u16,
    /// "If a nontoken creature would enter and it wasn't cast, exile it
    /// instead." (CR 614.1a)
    pub exiles_uncast_creatures: bool,
    /// "If damage would be dealt to this, instead that damage is dealt, but
    /// all other damage already dealt to it is healed." (CR 614.1a)
    pub heals_on_damage: bool,
    /// Alternative costs (CR 118.9; evoke, 702.74).
    pub alt_costs: Vec<AltCost>,
    /// "This spell costs {n} less to cast for each ..." (CR 601.2f).
    pub discounts: Vec<Discount>,
    /// `triggers[delayed_base..]` are delayed triggered abilities its
    /// effects create (CR 603.7).
    pub delayed_base: u8,
    /// Flashback cost (CR 702.34): castable from its owner's graveyard,
    /// then exiled.
    pub flashback: Option<ManaCost>,
    /// Escape (CR 702.138): its mana, and the rest (cards exiled from the
    /// graveyard).
    pub escape: Option<(ManaCost, Cost)>,
    /// "You may play lands from your graveyard."
    pub lands_from_graveyard: bool,
    /// "Combat damage that would be dealt by creatures you control can't
    /// be prevented."
    pub combat_unpreventable: bool,
    /// "As this enters, choose a player" (CR 614.12).
    pub chooses_player: bool,
    /// Delve (CR 702.66).
    pub delve: bool,
    /// Kicker (CR 702.33): `additional` is [not kicked, kicked] and the
    /// spell's modes are [its effect, its kicked effect], the mode given
    /// by the option (no mode choice).
    pub kicker: bool,
    /// "If this would enter, you may discard a land card instead. If you
    /// do, put it onto the battlefield; if you don't, into its owner's
    /// graveyard" (Mox Diamond; as it resolves).
    pub discard_land_to_enter: bool,
    /// {X} symbols in its mana cost (CR 107.3).
    pub x_pips: u8,
    /// "Enters with X +1/+1 counters" (the X it was cast with).
    pub enters_x_counters: bool,
    /// "You may play n additional lands on each of your turns."
    pub extra_lands: u8,
    /// Buyback (CR 702.27): `additional` is [none, buyback]; bought back,
    /// it goes to its owner's hand as it resolves.
    pub buyback: bool,
    /// Overload (CR 702.96): alternative cost 0; the spell's modes are
    /// [its targeted effect, its "each" effect], the mode given by the
    /// cost.
    pub overload: bool,
    /// "Each nonland card in your graveyard has escape": the rest of the
    /// escape cost (the mana cost is the card's).
    pub grants_escape: Option<Cost>,
    /// "Each player can't cast more than one spell each turn."
    pub one_spell_each_turn: bool,
    /// "Crews Vehicles as though its power were n greater."
    pub crew_bonus: i32,
    /// "<Types> your opponents control enter tapped" (card-type bits), and
    /// "nonbasic lands your opponents control enter tapped".
    pub opp_enter_tapped: u16,
    pub opp_nonbasic_enter_tapped: bool,
    /// Printed loyalty (CR 306.5a): a planeswalker enters with that many
    /// loyalty counters (306.5b).
    pub loyalty: Option<u8>,
    /// "As long as this entered this turn, you may activate its loyalty
    /// abilities any time you could cast an instant."
    pub loyalty_instant_if_entered: bool,
    /// "Each opponent can't draw more than one card each turn."
    pub opp_draw_limit: bool,
    /// "Each opponent can cast spells only any time they could cast a
    /// sorcery."
    pub opp_sorcery_speed: bool,
    /// "Spells and abilities your opponents control can't cause you to
    /// discard cards or sacrifice permanents."
    pub no_forced_discard_sac: bool,
    /// "If one or more tokens would be created under your control, n times
    /// that many of those tokens are created instead" (CR 614.1a; 0 =
    /// none).
    pub token_factor: u8,
    /// "If you would draw a card while your library has no cards in it, you
    /// win the game instead" (CR 614.1a).
    pub wins_on_empty_draw: bool,
    /// "Whenever you tap a <land subtype> for mana, add an additional
    /// <mana>" (a triggered mana ability, CR 605.1b): (subtype, mana type).
    pub extra_mana_on_tap: Vec<(u16, u8)>,
    /// An emblem's definition (CR 114): no characteristics but abilities;
    /// it lives in the command zone.
    pub emblem: bool,
    /// "<Cards matching> in your graveyard have retrace" (an emblem; CR
    /// 702.81).
    pub grants_retrace: Option<Filter>,
    // ---- cards with more than one face (M7b part 2; ENGINE.md 25) ----
    /// A double-faced card (CR 712): on the card's definition, its back
    /// face; on a back face, the card's definition (its front face).
    /// Transforming turns one into the other (CR 701.27). Face definitions
    /// are stored after the tokens.
    pub back: Option<DefId>,
    /// A modal double-faced card (CR 712.3): either face may be cast or
    /// played (712.11b, 712.12); its back face has its own mana cost and
    /// mana value (712.8f).
    pub modal: bool,
    /// An adventurer card's Adventure (CR 715.2): the alternative
    /// characteristics it may be cast with (an instant or sorcery face).
    pub adventure: Option<DefId>,
    /// On a face's definition (a back face, an Adventure): the card's
    /// definition, which is what the card is outside the battlefield and
    /// the stack (CR 712.8a, 715.4) and which gives its identity.
    pub front: Option<DefId>,
    /// The mana value of a nonmodal back face: its front face's (CR 712.8e,
    /// 202.3b); it has no mana cost of its own.
    pub front_mv: Option<u8>,
    /// Daybound (on a front face) and nightbound (on its back face) (CR
    /// 702.145).
    pub daybound: bool,
    pub nightbound: bool,
    /// A Saga's final chapter number (CR 714.2d); 0 = not a Saga.
    pub chapters: u8,
    /// "Your opponents can't cast spells during your turn."
    pub opp_no_spells_your_turn: bool,
    /// The last `latent` activated abilities are latent: a permanent has
    /// ability k of them only once an effect gave it (`Mod::GainLatent`).
    pub latent: u8,
    /// "You have no maximum hand size" (CR 402.2).
    pub no_max_hand: bool,
    /// A split card (CR 709): its left and right halves (faces). The card
    /// itself has the halves' characteristics combined (709.4) and is cast
    /// as one half (709.3).
    pub halves: Option<(DefId, DefId)>,
    /// "Spells you cast from your graveyard cost {n} less to cast."
    pub gy_spell_discount: u8,
    /// "Each nonland card in your graveyard has mayhem. The mayhem cost is
    /// equal to its mana cost." (CR 702.187)
    pub grants_mayhem: bool,
}

/// A cost reduction of a spell (CR 601.2f), in generic mana.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Discount {
    /// n less per basic land type among lands you control (domain).
    PerBasicLandType(u8),
    /// n less per permanent you control matching the filter (affinity).
    PerControlled(Filter, u8),
}

impl CardDef {
    pub fn is(&self, t: u16) -> bool {
        self.types & t != 0
    }
    pub fn is_permanent_card(&self) -> bool {
        self.types & types::PERMANENT != 0
    }
    pub fn mana_value(&self) -> u8 {
        // CR 712.8e: a nonmodal back face's mana value is its front face's.
        match self.front_mv {
            Some(mv) => mv,
            None => self.cost.mana_value(),
        }
    }
    /// The card's definition (its front face) of a face definition, else
    /// itself.
    #[inline]
    pub fn card_def(&self, me: DefId) -> DefId {
        self.front.unwrap_or(me)
    }
    /// The activated ability in `slot` (`Activated` or `Hand`).
    pub fn activated_def(&self, slot: AbilitySlot) -> &ActivatedDef {
        match slot {
            AbilitySlot::Activated(i) => &self.activated[i as usize],
            AbilitySlot::Hand(i) => &self.hand_abilities[i as usize],
            other => panic!("{other:?} is not an activated ability"),
        }
    }
}

/// All supported cards of a pool, plus the blockers of the unsupported ones.
#[derive(Debug, Default)]
pub struct CardDb {
    /// Card definitions, then token definitions from `token_base` on.
    defs: Vec<CardDef>,
    token_base: DefId,
    by_name: BTreeMap<String, DefId>,
    unsupported: BTreeMap<String, Vec<String>>,
    subtypes: Vec<String>,
}

impl CardDb {
    /// Lower every card; supported ones become definitions, the others are
    /// recorded with every feature that blocks them.
    pub fn from_cards(cards: &[card_ir::Card]) -> CardDb {
        let mut db = CardDb::default();
        let mut tokens: Vec<CardDef> = Vec::new();
        // The faces of each supported card (its back face, its Adventure).
        let mut faces: Vec<(DefId, Vec<CardDef>)> = Vec::new();
        for card in cards {
            match lower::lower_card(card, &mut db.subtypes, &mut tokens) {
                Ok((def, fs)) => {
                    let id = db.defs.len() as DefId;
                    assert!(
                        db.by_name.insert(card.name.clone(), id).is_none(),
                        "duplicate card name {}",
                        card.name
                    );
                    db.defs.push(def);
                    if !fs.is_empty() {
                        faces.push((id, fs));
                    }
                }
                Err(blockers) => {
                    db.unsupported.insert(card.name.clone(), blockers);
                }
            }
        }
        // Tokens the cards create (CR 111), interned during lowering.
        db.token_base = db.defs.len() as DefId;
        db.defs.extend(tokens);
        // Faces after the tokens: the card's `back` / `adventure` held the
        // face's index among its faces (`lower_card`); a face knows its card.
        for (id, fs) in faces {
            let base = db.defs.len() as DefId;
            let card = &mut db.defs[id as usize];
            let back = card.back.map(|k| base + k);
            let adventure = card.adventure.map(|k| base + k);
            card.back = back;
            card.adventure = adventure;
            card.halves = card.halves.map(|(a, b)| (base + a, base + b));
            for (k, mut f) in fs.into_iter().enumerate() {
                f.front = Some(id);
                if back == Some(base + k as DefId) {
                    f.back = Some(id); // a back face transforms into its front
                }
                db.defs.push(f);
            }
        }
        db
    }

    /// Is this a face's definition (a back face, an Adventure: CR 712, 715)
    /// rather than a card's, a token's or an emblem's?
    pub fn is_face(&self, id: DefId) -> bool {
        self.defs[id as usize].front.is_some()
    }

    /// The definition of token `t` (an index of `Op::CreateToken`).
    pub fn token_def(&self, t: u16) -> DefId {
        self.token_base + t
    }

    /// A token (or emblem) definition by name.
    pub fn token_by_name(&self, name: &str) -> Option<DefId> {
        (self.token_base as usize..self.defs.len()).find(|&i| self.defs[i].name == name).map(|i| i as DefId)
    }

    /// Number of card definitions (`0..num_cards()`); token definitions
    /// follow them.
    pub fn num_cards(&self) -> usize {
        self.token_base as usize
    }

    pub fn from_document(doc: &card_ir::Document) -> CardDb {
        CardDb::from_cards(&doc.cards)
    }

    /// Make the database `'static` (it is shared by every game and clone).
    pub fn leak(self) -> &'static CardDb {
        Box::leak(Box::new(self))
    }

    /// The checked-in PowerLSV cube, lowered once per process.
    pub fn cube() -> &'static CardDb {
        static CUBE: OnceLock<&'static CardDb> = OnceLock::new();
        CUBE.get_or_init(|| CardDb::from_document(&load_cube_document()).leak())
    }

    pub fn id(&self, name: &str) -> Option<DefId> {
        self.by_name.get(name).copied()
    }

    /// Like [`id`](Self::id) but explains why a card is missing.
    pub fn expect_id(&self, name: &str) -> Result<DefId, String> {
        if let Some(id) = self.id(name) {
            return Ok(id);
        }
        match self.unsupported.get(name) {
            Some(b) => Err(format!("{name} is not supported: {}", b.join(", "))),
            None => Err(format!("unknown card {name:?}")),
        }
    }

    #[inline]
    pub fn def(&self, id: DefId) -> &CardDef {
        &self.defs[id as usize]
    }

    pub fn ability(&self, r: AbilityRef) -> &AbilityDef {
        let d = self.def(r.def);
        let ab = match r.slot {
            AbilitySlot::Spell => d.spell.as_ref().expect("spell ability"),
            AbilitySlot::Trigger(i) => &d.triggers[i as usize].ability,
            AbilitySlot::Activated(_) | AbilitySlot::Hand(_) => match &d.activated_def(r.slot).effect {
                ActEffect::Stack(ab) => ab,
                ActEffect::Mana(_) => panic!("a mana ability has no stack program"),
            },
        };
        match r.mode {
            None => ab,
            Some(m) => &ab.modes.as_ref().expect("a modal ability").list[m as usize],
        }
    }

    pub fn len(&self) -> usize {
        self.defs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }

    /// Names of the supported cards, sorted.
    pub fn supported(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(|s| s.as_str())
    }

    /// Unsupported cards and their blockers, sorted by name.
    pub fn unsupported(&self) -> &BTreeMap<String, Vec<String>> {
        &self.unsupported
    }

    pub fn subtype_id(&self, name: &str) -> Option<u16> {
        self.subtypes.iter().position(|s| s == name).map(|i| i as u16)
    }

    pub fn subtype_name(&self, id: u16) -> &str {
        &self.subtypes[id as usize]
    }
}

/// Path of the checked-in cube IR.
pub fn cube_ir_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/cards/powerlsv.ir.json")
}

/// The checked-in cube IR, compiled in (so the Python module `mtgz._engine`
/// does not depend on where it was built).
pub const CUBE_IR: &str = include_str!("../../data/cards/powerlsv.ir.json");

pub fn load_cube_document() -> card_ir::Document {
    card_ir::load_document(CUBE_IR).unwrap_or_else(|e| panic!("{e}"))
}
