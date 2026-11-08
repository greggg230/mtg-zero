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
    /// Infect (CR 702.90): damage to players as poison counters, to
    /// creatures as -1/-1 counters. (Not in the encoding's keyword list,
    /// which layout v3 fixes: a card's printed keywords come with its
    /// identity.)
    Infect,
    /// Shadow (CR 702.28): blocks and is blocked only by creatures with
    /// shadow. (Not in the encoding's keyword list either.)
    Shadow,
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
    pub fn remove(&mut self, k: Kw) {
        self.0 &= !(1 << k as u32);
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
    /// From spells that are one or more colours (Emrakul, the Aeons Torn).
    pub colored_spells: bool,
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
        self.colored_spells |= o.colored_spells;
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
    /// Layer 7c: +p/+t for each permanent matching the filter (relative to
    /// the static ability's controller; a static ability only).
    PtPer { filter: Filter, p: i32, t: i32 },
    /// Layer 7c: +p/+t for each counter of this kind on the static
    /// ability's source ("equipped creature gets +1/+1 for each +1/+1
    /// counter on this Equipment").
    PtPerSourceCounter { counter: Counter, p: i32, t: i32 },
    /// Layer 6: it gains "Sacrifice this permanent: Add one mana of a type in
    /// the mask" (Rain of Filth; a mana ability used while paying, together
    /// with its own tap abilities: `pay::SAC_FOR`, ENGINE.md 31).
    GainSacMana(ManaMask),
    /// Layer 6: it loses all abilities (CR 613.1f).
    LoseAllAbilities,
    /// "Can attack as though it didn't have defender" (CR 702.3b: it keeps
    /// defender; Mechan Shieldmate, ENGINE.md 99.11).
    AttackDespiteDefender,
    /// It has all creature types (CR 205.3m; layer 4: Mutavault's "a 2/2
    /// creature with all creature types", ENGINE.md 99.11).
    AllCreatureTypes,
    /// Layer 4: its card types become exactly `types` and its creature
    /// types exactly `subtypes` (CR 205.1a: "becomes a [subtype] creature";
    /// supertypes are kept).
    BecomeTypes { types: u16, subtypes: Vec<u16> },
    /// Layer 6: it gains latent activated ability k of its card ("this
    /// Saga gains '<ability>'"; `CardDef::latent`).
    GainLatent(u8),
    /// Layer 7d: its power and toughness are switched (CR 613.4d; an even
    /// number of switches cancel). ENGINE.md 99.3.
    SwitchPt,
    /// Goaded (CR 701.38) by the effect's controller: it attacks each combat
    /// if able, and a player other than the goader if able.
    Goad,
    /// "If it would leave the battlefield, exile it instead of putting it
    /// anywhere else" (CR 614.1a).
    ExileIfLeaves,
    /// Layer 2: its controller gained control of it from player `.0` until
    /// end of turn (CR 613.1b); as the effect ends, control returns
    /// (`Game::end_turn_effects`; EoE lane D). No characteristic changes.
    ControlFrom(u8),
    /// Combat damage it would deal is prevented (CR 615.1a; "prevent all
    /// combat damage that would be dealt this turn" on every creature, a
    /// rules effect; EoE lane D).
    PreventCombatDamage,
    /// "It attacks that player this combat if able" (the effect
    /// controller's opponent, 2 players: never one of their planeswalkers;
    /// Ursine Monstrosity).
    MustAttackPlayer,
    /// Layer 7c: +X/+0, X the greatest mana value among the other
    /// permanents of these card types the static ability's controller
    /// controls (a static ability of the permanent itself; Emissary Escort,
    /// ENGINE.md 99.2).
    PowerPlusGreatestMv { types: u16 },
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
    /// Power and toughness = the lands you control (ENGINE.md 99.3).
    LandsYouControl,
    /// Power = the cards its controller owns in exile (face up or down;
    /// not tokens, CR 108.2); toughness = that plus `plus` (Cosmogoyf).
    OwnedInExile { plus: i32 },
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
    /// Mana value at most the number of colours of mana spent to cast the
    /// spell that is the context's source (converge, CR 702.?/Prismatic
    /// Ending; ENGINE.md 28).
    MvAtMostConverge,
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
    /// A blocking creature (CR 509.1g; ENGINE.md 99.4).
    Blocking,
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
    /// A permanent attached to a permanent matching the inner filter (an
    /// Aura's enchanted creature, CR 303.4b).
    AttachedTo(Box<Filter>),
    /// An ability on the stack of these kinds (`ABILITY_*` bits: activated,
    /// triggered; CR 113.1, 115.1: "target activated or triggered
    /// ability"). Mana abilities never use the stack (605.3b).
    Ability(u8),
    /// It has been harnessed (`Mark::Harnessed`; "once harnessed, its
    /// infinity ability is active").
    Harnessed,
    /// Mana value at most the X of the resolving spell that is the context's
    /// source (CR 107.3).
    MvAtMostX,
    /// Mana value at most the X its source paid as it was cast (kicker {X};
    /// `Mark::KickerX` of the source permanent).
    MvAtMostSourceX,
    /// Its name is this ("creatures named ...").
    Named(String),
    /// Its power and toughness are exactly these (CR 208).
    PtIs(i32, i32),
    /// Saddled this turn (CR 702.171b).
    Saddled,
    /// Has an activated ability (CR 602; mana abilities, loyalty abilities
    /// and activated keyword abilities included; special actions, CR 116,
    /// are not abilities). A companion condition (Zirda, the Dawnwaker)
    /// reads it over the cards of a starting deck (`DeckCond`).
    HasActivatedAbility,
    /// A permanent that was dealt damage this turn (`Mark::DealtDamage`; EoE
    /// lane D).
    DealtDamageThisTurn,
    /// Mana value at most the number of lands the controller controls (EoE
    /// lane D).
    MvAtMostLandsYouControl,
    /// A permanent that entered as a spell cast for its warp cost (CR
    /// 702.185a; `Mark::WarpCast`; ENGINE.md 99.8).
    CastForWarp,
}

/// `Filter::Ability` kinds.
pub const ABILITY_ACTIVATED: u8 = 1;
pub const ABILITY_TRIGGERED: u8 = 2;

/// Can [`CardDb::def_matches`] answer `f` (a filter on a card's own
/// characteristics, outside any game)?
pub fn deck_filter_ok(f: &Filter) -> bool {
    match f {
        Filter::Any
        | Filter::Type(_)
        | Filter::Supertype(_)
        | Filter::Color(_)
        | Filter::Colorless
        | Filter::ManaValueAtMost(_)
        | Filter::ManaValueAtLeast(_)
        | Filter::Named(_)
        | Filter::HasKeyword(_)
        | Filter::HasActivatedAbility => true,
        Filter::And(a, b) => deck_filter_ok(a) && deck_filter_ok(b),
        Filter::Or(v) => v.iter().all(deck_filter_ok),
        Filter::Not(x) => deck_filter_ok(x),
        _ => false,
    }
}

/// A companion's condition on its owner's starting deck (CR 702.139a; the
/// deck after the sideboard is set aside, 103.2a, 702.139b; ENGINE.md 35).
/// Its filters read a card's own characteristics (`CardDb::def_matches`):
/// cards outside the battlefield and the stack have their front face's
/// (712.8a), an adventurer card its normal ones (715.4).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum DeckCond {
    /// "Each <cards> in your starting deck <must>" (Lurrus of the Dream-Den,
    /// Zirda, the Dawnwaker).
    Each { cards: Filter, must: Filter },
    /// "Each <cards> in your starting deck has a different name" (Lutri, the
    /// Spellchaser; CR 201.2).
    DifferentNames { cards: Filter },
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TargetReq {
    pub filter: Filter,
    pub min: u8,
    pub max: u8,
    /// Bit k: the targets of this requirement must differ from those
    /// chosen for requirement k ("another target", CR 115.3).
    pub distinct: u8,
    /// k + 1: a card owned by the controller of the target chosen for
    /// requirement k ("target artifact card in that player's graveyard",
    /// Goblin Welder); 0 = no such link.
    pub pair: u8,
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
    /// Spree (CR 702.172): the additional cost of each mode, paid for each
    /// mode chosen (700.2h); empty for other modal spells.
    pub costs: Vec<ManaCost>,
    /// "Choose one that hasn't been chosen" (Gollum): a mode chosen once for
    /// this ability of its source object is not offered again
    /// (`Mark::ModesUsed`).
    pub once_each: bool,
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
    /// A player, a permanent or a spell became the target of a spell or
    /// ability ("becomes the target", CR 603.2e, 702.21a ward) as its
    /// targets were chosen (601.2c, 602.2b, 603.3d) or changed (115.7,
    /// 707.10c). Subjects: target (a player or a permanent) or target
    /// spell (a spell: the event's amount is 1), then controller: the
    /// spell or ability that targets (`Subj::Item`; its controller).
    BecomesTarget,
    /// A player played a land (CR 305.1; not a land put onto the
    /// battlefield by an effect). Subjects: the land, the player.
    PlaysLand,
    /// A card was put into exile from its owner's library or graveyard.
    /// Subject: the owner.
    ExiledFromLibGy,
    /// A card with "when this is put into a graveyard from anywhere" was
    /// put into a graveyard (emitted only for such cards; its own ability
    /// sees it there). Subject: the card in the graveyard.
    ToGraveyard,
    /// One or more +1/+1 counters were put on a permanent (CR 122.6).
    /// Subject: the permanent.
    CountersAdded,
    /// One or more cards were put into a library from anywhere ("one or
    /// more": once among the triggers waiting). Subject: the library's owner.
    CardsPutIntoLibrary,
    /// Creatures a player controls attack another and some aren't blocked
    /// (CR 509.1h). Subjects: the attacking player, the defending player.
    AttackersUnblocked,
    /// A Class became level `amount` (CR 716.2c). Subject: obj.
    ClassLevel,
    /// A card left a graveyard for another zone ("whenever an instant or
    /// sorcery card leaves your graveyard", Murktide Regent; ENGINE.md 54).
    /// Recorded before the card moves (CR 603.10a: it looks back in time).
    /// Subjects: the graveyard's owner, the card (as it was there).
    LeavesGraveyard,
}

/// When a delayed triggered ability triggers (CR 603.7).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DelayedWhen {
    /// At the beginning of the next end step.
    NextEndStep,
    /// At the beginning of the next cleanup step (CR 514.3a).
    NextCleanup,
    /// At the beginning of the next turn's upkeep.
    NextUpkeep,
    /// At the beginning of its controller's next main phase.
    YourNextMainPhase,
    /// At the beginning of the end step of its controller's next turn (not
    /// the turn it was created in: Kav Landseeker's "on your next turn",
    /// ENGINE.md 99.3).
    YourNextTurnEndStep,
    /// At the next end of combat ("at end of combat": as the end of combat
    /// step begins, CR 511.2; EoE lane D, ENGINE.md 99.5).
    EndOfCombat,
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
            // (The blocked attacker: "whenever this creature blocks a
            // creature with flying", CR 509.3b; EoE lane D.)
            E::Blocks => (&[Slot::Blocker], &[Slot::Attacker]),
            E::Transforms | E::Chapter | E::ClassLevel => (&[Slot::Obj], &[]),
            E::DealsDamage => (&[Slot::Source], &[]),
            E::YouAttack
            | E::DrawsCard
            | E::BeginningOfUpkeep
            | E::BeginningOfDrawStep
            | E::BeginningOfPrecombatMain
            | E::BeginningOfCombat
            | E::BeginningOfEndStep
            | E::CreatureTokensCreated
            | E::ExiledFromLibGy => (&[Slot::Player], &[]),
            E::ToGraveyard | E::CountersAdded => (&[Slot::Obj], &[]),
            E::CardsPutIntoLibrary => (&[Slot::Player], &[]),
            E::AttackersUnblocked => (&[Slot::Player], &[Slot::Defender]),
            E::DealsCombatDamage | E::DealsCombatDamageToCreature => (&[Slot::Source], &[Slot::Target]),
            E::CardDiscarded | E::LeavesGraveyard => (&[Slot::Player], &[Slot::Card]),
            E::Delayed => (&[], &[]),
            E::IsCast => (&[Slot::Spell], &[Slot::Controller]),
            E::BecomesTarget => (&[Slot::Target, Slot::TargetSpell], &[Slot::Controller]),
            E::PlaysLand => (&[Slot::Land, Slot::Obj], &[Slot::Controller, Slot::Player]),
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
    /// A spell that became the target ("a creature spell you control
    /// becomes the target"; `BecomesTarget`).
    TargetSpell,
    /// The land a player played (`PlaysLand`).
    Land,
    /// The defending player (attackers unblocked).
    Defender,
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
    /// "When this Class becomes level n" (CR 716.2c: a `ClassLevel` event of
    /// the source with amount n).
    Level(u8),
    /// "This ability triggers only n times each turn", the ability a static
    /// ability of the source grants to the subject (Nadu): per subject
    /// object (`Mark::Triggered`).
    TimesPerSubject(Slot, u8),
    /// "Whenever one or more ...": triggers once for the events of one
    /// simultaneous move (a batch, `Game::begin_batch`), however many match.
    Batch,
    /// An `Enters` event of a permanent that entered as a resolving spell
    /// cast by its controller ("if you cast it"; the event's note,
    /// `Game::enter_note`).
    EnteredCast,
    /// An `Enters` event of a permanent whose spell had an additional cost
    /// paid one or more times (squad, offspring: CR 702.157a, 702.175a).
    EnteredPaid,
    /// An `IsCast` event of a spell whose additional cost was paid one or
    /// more times (replicate, CR 702.56a).
    SpellPaid,
    /// "Whenever you cast or copy": an `IsCast` event of a copy of a spell
    /// matches too (CR 707.10: a copy isn't cast; only these abilities see
    /// it).
    OrCopy,
    /// The subject (a permanent) has the abilities the source's static
    /// abilities grant it: it hasn't lost all its abilities in an effect
    /// later than the source's timestamp (CR 613.7; a granted ward, whose
    /// triggered ability is the granter's here, ENGINE.md 27.3).
    Granted(Slot),
    /// The attacker attacks for the first time this turn (it was declared
    /// as an attacker once: additional combat phases, CR 500.8).
    FirstAttack,
    /// "If X is n or more" of ravenous (CR 702.156a): the X of the spell
    /// that became the entering permanent (its `Enters` event's note).
    EnteredXAtLeast(u8),
    /// "This ability triggers only once each turn" (always true; once it
    /// has triggered, it doesn't trigger again that turn, `triggers.rs`).
    Once,
    /// "Enters from anywhere other than a graveyard or exile" (the Enters
    /// event's note, bit 15).
    EnteredNotFromGyExile,
    /// The ability functions in its card's graveyard, and only there
    /// ("return this card from your graveyard", CR 113.6m-like).
    InGraveyard,
    /// The event's spell's mana value has the parity chosen as the source
    /// entered (`Mark::Parity`).
    SpellMvParity,
    /// The attacker attacks the monarch (the player, not a planeswalker;
    /// CR 724, 508.1b).
    AttacksMonarch,
    /// A `LeavesBattlefield` event of an object that went to one of these
    /// zones (`left_to` bits; "dies or is put into exile", CR 700.4:
    /// `Game::leaving_to` as the event's triggers are matched; ENGINE.md
    /// 99.11).
    LeftTo(u8),
}

/// The zone a permanent left the battlefield for (`Game::leaving_to`,
/// `TrigPred::LeftTo`).
pub mod left_to {
    use crate::ids::Zone;
    pub const GRAVEYARD: u8 = 1;
    pub const EXILE: u8 = 2;
    pub const HAND: u8 = 4;
    pub const LIBRARY: u8 = 8;
    /// Anywhere else (a token or a copy ceasing to exist, CR 111.7).
    pub const OTHER: u8 = 16;

    pub fn bit(z: Zone) -> u8 {
        match z {
            Zone::Graveyard => GRAVEYARD,
            Zone::Exile => EXILE,
            Zone::Hand => HAND,
            Zone::Library => LIBRARY,
            _ => OTHER,
        }
    }
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
    /// `CardDef.gy_abilities[i]` (activated from its owner's graveyard,
    /// ENGINE.md 27.5).
    Graveyard(u8),
}

/// A condition, evaluated for a player (the ability's controller): "if",
/// "activate only if", "enters tapped unless".
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Cond {
    /// The ability's source is attached to a creature (reconfigure's
    /// unattach, CR 702.151a).
    SourceAttached,
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
    /// An artifact entered the battlefield under your control this turn
    /// (Mechan Shieldmate; whether it stayed doesn't matter, its ruling of
    /// 2025-07-25; ENGINE.md 99.11).
    ArtifactEnteredThisTurn,
    /// A player controls no creatures (Sothera, the Supervoid; ENGINE.md
    /// 99.11).
    APlayerControlsNoCreatures,
    /// You gained n or more life this turn (EoE lane D).
    GainedLifeAtLeast(u16),
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
    /// You have cast at least n spells this turn ("if you've cast another
    /// spell this turn" while casting one: 2).
    SpellsCastAtLeast(u8),
    /// You have the initiative (CR 725).
    HasInitiative,
    /// You have completed a dungeon (CR 309.7).
    CompletedDungeon,
    /// All of these hold (a static ability's own conditions included).
    All(Vec<Cond>),
    /// There are n or more card types among the cards exiled with the
    /// static ability's source (CR 607.2a, 205.2a; a static's own
    /// condition, read in `layers.rs`).
    LinkedTypesAtLeast(u8),
    /// You have the city's blessing (CR 702.131).
    CityBlessing,
    /// You played at least n lands this turn (Fastbond: "if it wasn't the
    /// first land you played this turn").
    LandsPlayedAtLeast(u8),
    /// An opponent controls more lands than you (ENGINE.md 99.3).
    OpponentControlsMoreLands,
    /// Void: a nonland permanent left the battlefield this turn or a spell
    /// was warped this turn (Edge of Eternities' ability word; ENGINE.md
    /// 99.4).
    Void,
    /// Your opponents control at least n permanents matching the filter
    /// between them ("if your opponents control three or more creatures").
    OpponentsControlAtLeast(Filter, u8),
    /// You attacked with a creature of this subtype (its card's printed
    /// subtypes) this turn ("if you attacked with a Spacecraft this turn").
    AttackedWithSubtype(u16),
}

impl Cond {
    /// Does it read the cards in `zone` (their number or kinds)? A spell
    /// leaving that zone for the stack can change it as its targets are
    /// chosen (CR 601.2a before 601.2c; `Game::as_cast`). Exhaustive: a new
    /// condition says.
    pub(crate) fn reads_zone(&self, zone: crate::ids::Zone) -> bool {
        use crate::ids::Zone;
        match self {
            Cond::HandExactly(_) | Cond::HandAtMost(_) | Cond::OpponentHasMoreCards => zone == Zone::Hand,
            Cond::GraveyardAtLeast(_) | Cond::Delirium(_) => zone == Zone::Graveyard,
            Cond::DevotionCoversLibrary(_) | Cond::LibraryEmpty => zone == Zone::Library,
            // (The cards exiled with the static ability's source.)
            Cond::LinkedTypesAtLeast(_) => zone == Zone::Exile,
            Cond::All(v) => v.iter().any(|c| c.reads_zone(zone)),
            Cond::SourceAttached
            | Cond::ControlAtLeast(..)
            | Cond::ControlAtMost(..)
            | Cond::Revolt
            | Cond::YourTurnAtMost(_)
            | Cond::FirstSpellThisGame
            | Cond::IsMonarch
            | Cond::SourceCountersIn(..)
            | Cond::GainedLifeThisTurn
            | Cond::ArtifactEnteredThisTurn
            | Cond::APlayerControlsNoCreatures
            | Cond::GainedLifeAtLeast(_)
            | Cond::SpellsCastThisTurn { .. }
            | Cond::YourTurn
            | Cond::LifeAtMostHalfStart
            | Cond::ControlOtherOfColor(_)
            | Cond::SpellsCastAtLeast(_)
            | Cond::HasInitiative
            | Cond::CompletedDungeon
            | Cond::CityBlessing
            | Cond::LandsPlayedAtLeast(_)
            | Cond::OpponentControlsMoreLands
            | Cond::Void
            | Cond::OpponentsControlAtLeast(..)
            | Cond::AttackedWithSubtype(_) => false,
        }
    }
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
    /// X mana in any combination of types in the mask, X announced as the
    /// ability is activated (its cost's {X}, CR 107.3; Wizard's Rockets).
    /// With priority only (ENGINE.md 28.1): one `ChooseMana` per mana.
    ComboX { mask: ManaMask },
    /// One mana of type `ty` per permanent you control matching the filter
    /// (Gaea's Cradle).
    PerPermanent { ty: u8, filter: Filter },
    /// One mana of any colour among permanents you control matching the
    /// filter (Mox Amber).
    ColorAmong(Filter),
    /// Fixed amounts `then` if the condition holds for you, else `els`
    /// ("add {G}{G}; if you've completed a dungeon, add six {G} instead").
    IfElse { cond: Cond, then: [u8; 6], els: [u8; 6] },
    /// One mana of any of the colours of the cards exiled with the source
    /// (CR 607.2a; Chrome Mox's imprint).
    Linked,
    /// One of these fixed productions (Sunken Ruins: {U}{U}, {U}{B} or
    /// {B}{B}).
    OneOf(Vec<[u8; 6]>),
    /// One mana of any type the land sacrificed for its cost could produce
    /// (Squandered Resources; CR 106.7, as the land last existed).
    SacrificedLand,
    /// One mana of the colour of the basic land type chosen as the source
    /// entered (CR 305.6: "this land is the chosen type").
    Chosen,
    /// One mana of a type in `then` if the source has a counter of this
    /// kind, else in `els` (Gemstone Caverns: "add {C}. If it has a luck
    /// counter on it, instead add one mana of any color").
    IfCounter { counter: Counter, then: ManaMask, els: ManaMask },
    /// One mana of any type a land you control could produce (CR 106.7;
    /// Reflecting Pool, ENGINE.md 99.3): the types the mana abilities of
    /// those lands make, other abilities of this kind making none of their
    /// own.
    LandsCouldProduce,
}

/// A mana ability's effect: the mana, plus damage its source deals to its
/// controller (Talismans, Ancient Tomb).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ManaAbility {
    pub prod: ManaProd,
    pub damage_to_you: u8,
    /// "If there are no <kind> counters on this, sacrifice it" (Gemstone
    /// Mine), after the mana.
    pub sac_if_no: Option<Counter>,
    /// "Spend this mana only to cast artifact spells" / "... a legendary
    /// spell" (`SPEND_*` bits; 0 = no restriction, CR 106.6).
    pub only: u8,
    /// "... and that spell can't be countered" (Delighted Halfling).
    pub uncounterable: bool,
    /// "If any of that mana is spent on a creature spell, it gains haste
    /// until end of turn" (Arena of Glory; the pool's `haste` mana).
    pub haste: bool,
    /// "Each opponent gains n life" (Grove of the Burnwillows; ENGINE.md
    /// 99.3), as the mana is added.
    pub opp_life: u8,
}

/// `ManaAbility::only`: the mana may be spent only to cast an artifact
/// spell.
pub const SPEND_ARTIFACT: u8 = 1;
/// ... only to cast a legendary spell.
pub const SPEND_LEGENDARY: u8 = 2;

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
    /// `crew` is saddle's (CR 702.171a): "crews Vehicles as though its
    /// power were greater" doesn't apply.
    pub saddle: bool,
    /// Tap n untapped permanents you control matching the filter ("tap an
    /// untapped Rebel you control"; not {T}: summoning sickness doesn't
    /// matter, CR 302.6).
    pub tap_untapped: Option<SacCost>,
    /// Waterbend: untapped artifacts and creatures may each pay {1} of the
    /// mana (`PayMana::waterbend`).
    pub waterbend: bool,
    /// Exert this permanent (CR 701.39: it won't untap during its
    /// controller's next untap step; `Mark::Exerted`).
    pub exert: bool,
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
    /// Bestow (CR 702.103): cast bestowed, as the card's bestowed face
    /// (`CardDef::bestow`), an Aura spell.
    pub bestow: bool,
    /// Warp (CR 702.185): `dash`'s delayed ability is warp's "exile it at
    /// the beginning of the next end step" (no haste).
    pub warp: bool,
    /// Emerge (CR 702.119): sacrifice a permanent matching the filter; the
    /// cost is reduced by its mana value. The permanent is chosen before the
    /// mana is paid (`Task::Emerge`, `StackItem::noted`).
    pub emerge: Option<Filter>,
}

/// An additional cost that may be paid a number of times as a spell is cast
/// (CR 601.2b): replicate (702.56a), squad (702.157a), offspring (702.175a:
/// once). The number paid is `StackItem::times`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TimesCost {
    pub mana: ManaCost,
    /// At most this many times (1 for offspring).
    pub max: u8,
    /// The cost has {X} (kicker {X}: X announced when it is paid, at least
    /// `CardDef::x_min`; the stack item's `x`).
    pub x: bool,
}

/// "You may have this enter as a copy of ..." (CR 707.5, 614.1c): what it
/// may copy and the copy's exceptions (707.9).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct EntersAsCopy {
    /// Permanents on the battlefield matching it (other than itself), or,
    /// with `from_graveyard`, cards in any graveyard matching it.
    pub filter: Filter,
    pub from_graveyard: bool,
    /// The exceptions (707.9b): card types and subtypes in addition, base
    /// power and toughness, keywords.
    pub exc: crate::copies::Except,
    /// "Except his name is <its own name>".
    pub keep_name: bool,
    /// "When you do, exile that card" (603.11, 707.9g): its delayed
    /// ability k (`delayed_base + k`), about the card copied.
    pub exile_k: Option<u8>,
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
    /// priority, never while paying a cost (Lion's Eye Diamond; ENGINE.md
    /// 18).
    pub instant_only: bool,
    /// Exhaust (CR 702.177): each object activates it only once.
    pub exhaust: bool,
    /// "Activate only if ..." (CR 602.5b).
    pub only_if: Option<Cond>,
    /// "Activate only during your turn."
    pub your_turn: bool,
    /// A loyalty ability (CR 606.2): the change in loyalty its cost makes
    /// (+1 = put a loyalty counter, -3 = remove three; 606.4).
    pub loyalty: Option<i8>,
    /// "This ability costs {n} less to activate for each ..." (CR 602.2b,
    /// 601.2f; Otawara's channel).
    pub discounts: Vec<Discount>,
    /// Boast (CR 702.142a): "activate only if this creature attacked this
    /// turn and only once each turn" (`Mark::Boasted`).
    pub boast: bool,
    /// Its mana cost has {X} (CR 107.3, 602.2b: X announced as it is
    /// activated; the stack item's `x`).
    pub x: bool,
    /// Not an activated ability but a special action (CR 116.2) offered the
    /// same way: no stack; it happens as soon as its cost is paid (M7d,
    /// ENGINE.md 31).
    pub special: Option<Special>,
}

/// A special action (CR 116.2) a card offers like an activated ability.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Special {
    /// Plot (CR 702.170a, 116.2k): exile the card from hand; its owner may
    /// cast it on a later turn without paying its mana cost.
    Plot,
    /// Turn a face-down permanent face up (CR 116.2b, 701.40b, 708.7).
    TurnFaceUp,
}

/// `TargetReq::max` meaning "X targets": exactly the announced X (CR
/// 601.2c with 107.3; "untap X target lands").
pub const X_TARGETS: u8 = u8::MAX;
/// `TargetReq::max` for "up to X targets" (Pest Infestation): between `min`
/// and the spell's X.
pub const X_UP_TO: u8 = u8::MAX - 1;
/// `CardDef::extra_lands` for "any number of lands" (Fastbond).
pub const ANY_LANDS: u8 = u8::MAX;
/// `TargetReq::max` for "up to X targets, where X is the number of times it
/// was kicked" (Batroc: an enter trigger, X from its event's note).
pub const KICKS_UP_TO: u8 = u8::MAX - 2;

impl CardDef {
    /// `game::SEEN_*` bits of the rare rules this definition has.
    pub(crate) fn seen_bits(&self) -> u16 {
        (if self.ascend { crate::game::SEEN_ASCEND } else { 0 })
            | (if self.ability_discount > 0 { crate::game::SEEN_ABILITY_DISCOUNT } else { 0 })
            | (if self.outside_hand_discount > 0 { crate::game::SEEN_OUTSIDE_DISCOUNT } else { 0 })
            | (if self.grants_retrace.is_some() && !self.emblem { crate::game::SEEN_RETRACE } else { 0 })
            | (if self.trig_mask & (1u64 << EventKind::ExiledFromLibGy as u32) != 0 { crate::game::SEEN_EXILE_WATCH } else { 0 })
            | (if self.opp_draw_token.is_some() || self.draw_plus.is_some() { crate::game::SEEN_DRAW_REPL } else { 0 })
            | (if self.gy_triggers { crate::game::SEEN_GY_TRIGGER } else { 0 })
            | (if self.void_exile.is_some() { crate::game::SEEN_VOID } else { 0 })
            | (if self.named_spell_tax > 0 || self.named_ability_tax > 0 { crate::game::SEEN_NAMED_TAX } else { 0 })
            | (if self.look_top || self.play_top_life { crate::game::SEEN_TOP } else { 0 })
            | (if self.spell.as_ref().is_some_and(|ab| {
                ab.program.iter().any(|op| matches!(op, Op::Ext(crate::program::Ext::Apply { mods, .. }) if mods.iter().any(|m| matches!(m, Mod::GainSacMana(_)))))
            }) {
                crate::game::SEEN_SAC_MANA
            } else {
                0
            })
            | (if self.triggers.iter().any(|t| t.ability.program.iter().any(|op| matches!(op, Op::Ext(crate::program::Ext::DamageFrom { event_obj: true, .. })))) {
                crate::game::SEEN_LKI_DAMAGE
            } else {
                0
            })
            | (if self.trig_mask & (1u64 << EventKind::CountersAdded as u32 | 1u64 << EventKind::CardsPutIntoLibrary as u32 | 1u64 << EventKind::AttackersUnblocked as u32) != 0 {
                crate::game::SEEN_RARE_EVENTS
            } else {
                0
            })
            | (if self.trig_mask & (1u64 << EventKind::LeavesGraveyard as u32) != 0 { crate::game::SEEN_GY_LEAVE } else { 0 })
    }
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
    /// An Aura's "enchanted creature can't be blocked by more than one
    /// creature" (CR 509.1b; Meltstrider's Resolve, ENGINE.md 99.11).
    pub attached_max_one_blocker: bool,
    /// "Can block only creatures with flying."
    pub blocks_only_flyers: bool,
    /// "Can't be blocked by" creatures matching this filter.
    pub unblockable_by: Option<Filter>,
    /// "Doesn't untap during your untap step" (CR 502.3).
    pub no_untap: bool,
    /// Counters it enters the battlefield with (CR 122.6).
    pub enters_counters: Vec<(Counter, u8)>,
    /// Bit k set: one of its triggered abilities listens for `EventKind` k.
    pub trig_mask: u64,
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
    /// "This creature enters with a +1/+1 counter on it for each <filter>
    /// card exiled with it" (Murktide Regent; ENGINE.md 54): the cards
    /// exiled to pay the cost of the spell that became it (its delve, CR
    /// 607.2q; `Mark::ExiledToPay`), as they are in exile (a card's own
    /// characteristics, `CardDb::def_matches`); 614.1c.
    pub counters_per_exiled: Option<Filter>,
    /// "This creature enters with a +1/+1 counter on it for each other
    /// creature and/or artifact you control" (Luxknight Breacher; CR
    /// 614.1c, 122.6): the permanents its controller controls that match,
    /// counted as it enters, not those entering at the same time (the card's
    /// ruling of 2025-07-25; ENGINE.md 99.11).
    pub enters_counters_per: Option<Filter>,
    /// "You may play n additional lands on each of your turns" (`ANY_LANDS`:
    /// any number).
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
    /// "If this would be put into a graveyard from anywhere, reveal it and
    /// shuffle it into its owner's library instead" (Blightsteel Colossus).
    pub gy_to_library: bool,
    /// Hullbreacher: an opponent's draw, but the first in their draw step,
    /// becomes this token for the controller (CR 614.1a).
    pub opp_draw_token: Option<u16>,
    /// Quantum Riddler: (d, n): with n or fewer cards in hand, its
    /// controller draws d more cards with each draw instruction.
    pub draw_plus: Option<(u8, u8)>,
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
    // ---- copies (M7b part 3; ENGINE.md 26) ----
    /// "You may have this enter as a copy of ..." (CR 707.5).
    pub enters_as_copy: Option<Box<EntersAsCopy>>,
    /// An additional cost paid any number of times, or once (replicate,
    /// squad, offspring).
    pub times_cost: Option<TimesCost>,
    /// Bestow (CR 702.103): the card's bestowed face, an Aura enchantment
    /// with enchant creature (702.103b; a face definition, `front` = the
    /// card). On that face: `bestowed`.
    pub bestow: Option<DefId>,
    pub bestowed: bool,
    /// A preparation card's prepare spell (CR 722.2a): a face (an instant or
    /// sorcery) that copies of the card in exile have (722.3c).
    pub prepare: Option<DefId>,
    /// "This creature enters prepared" (CR 722.3a).
    pub enters_prepared: bool,
    // ---- M7b part 3b: the stack, ward, the initiative, graveyard
    // abilities (ENGINE.md 27) ----
    /// "Spells you control can't be countered."
    pub spells_uncounterable: bool,
    /// Ward (CR 702.21) and its cost; the ward triggered ability itself is
    /// among `triggers` (synthesized by the lowering).
    pub ward: Option<WardCost>,
    /// "Other creatures you control have ward—<cost>" (its triggered
    /// abilities are this card's: ENGINE.md 27.3).
    pub grants_ward: Option<WardCost>,
    /// Improvise (CR 702.126): untapped artifacts its controller taps while
    /// paying pay for {1} each.
    pub improvise: bool,
    /// A definition of the rules, not of a card or token: a dungeon (CR
    /// 309), the initiative's inherent abilities (725.2).
    pub rules: bool,
    /// Abilities activated from its owner's graveyard (CR 602.2, 113.6:
    /// "exile this card from your graveyard", eternalize). At most one.
    pub gy_abilities: Vec<ActivatedDef>,
    /// "Activated abilities of artifacts you control cost {n} less to
    /// activate. This effect can't reduce the mana in that cost to less than
    /// one mana." (Forensic Gadgeteer.)
    pub artifact_ability_discount: u8,
    // ---- M7c: the single-card tail (ENGINE.md 28) ----
    /// "If you would begin your turn while this artifact is tapped, you may
    /// skip that turn instead. If you do, untap this artifact." (Time
    /// Vault; CR 614.1b, 614.10.)
    pub skip_turn_untap: bool,
    /// Its spell reads the colours of mana spent to cast it (converge): the
    /// payment records them (`Mark::Converge`).
    pub converge: bool,
    /// Sunburst (CR 702.44a): it enters with a +1/+1 counter (a creature) or
    /// this counter (charge) for each colour of mana spent to cast it.
    pub sunburst: Option<Counter>,
    /// "Enters with a +1/+1 counter on it for each time it was kicked"
    /// (multikicker, CR 702.33c).
    pub enters_times_counters: bool,
    /// The least X its kicker {X} may have ("X can't be 0").
    pub x_min: u8,
    /// Some triggered ability of it functions in its owner's graveyard
    /// (`TrigPred::InGraveyard`).
    pub gy_triggers: bool,
    /// "As this enters, choose odd or even" (Gollum).
    pub chooses_parity: bool,
    /// "This can't block or be blocked by non-<subtype> creatures" (Wan Shi
    /// Tong's Spirits).
    pub blocks_only_subtype: Option<u16>,
    /// A reanimating Aura (Animate Dead, Necromancy): attached to the
    /// creature it put onto the battlefield (`Mark::Reanimated`), or, before
    /// that, to the creature card in a graveyard it enchants; otherwise it is
    /// put into its owner's graveyard (CR 704.5m).
    pub reanimates: bool,
    /// An Aura spell's target (CR 303.4a; Animate Dead: "enchant creature
    /// card in a graveyard"): its `spell` holds the requirement.
    pub aura_spell: bool,
    /// "If you cast it any time a sorcery couldn't have been cast, ...
    /// sacrifices it at the beginning of the next cleanup step" (Necromancy):
    /// that delayed ability's index.
    pub cleanup_sac: Option<u8>,
    /// "You may cast this spell as though it had flash" (Necromancy): a
    /// permission, not the flash keyword (CR 702.8a; ir_defects 8).
    pub flash_permission: bool,
    /// Splice onto <subtype> <cost> (CR 702.47): the subtype, the cost.
    pub splice: Option<(u16, ManaCost)>,
    /// "As this enters, choose a color. All cards that aren't on the
    /// battlefield, spells, and permanents are the chosen color in addition
    /// to their other colors" (Painter's Servant; layer 5, CR 613.1e).
    pub paints: bool,
    /// "Whenever you tap a creature for mana, add an additional <mana>"
    /// (Badgermole Cub; a triggered mana ability, CR 605.1b): per type.
    pub extra_mana_creature: [u8; 6],
    /// "Once during each of your turns, you may play a land from your
    /// graveyard or cast a permanent spell with mana value n or less from
    /// your graveyard" (Serra Paragon; 0 = none) ...
    pub gy_play_mv: u8,
    /// ... "If you do, it gains '<delayed ability k>'" (its trigger
    /// `delayed_base + k`; `NO_GRANT`: nothing).
    pub gy_play_k: u8,
    /// ... and a land may be played this way (Serra Paragon; not Lurrus of
    /// the Dream-Den, "a permanent spell").
    pub gy_play_land: bool,
    // ---- M7c: the single-card tail, part 2 (ENGINE.md 29) ----
    /// "Abilities you activate that aren't mana abilities cost {n} less to
    /// activate. This effect can't reduce the mana in that cost to less than
    /// one mana." (Zirda, the Dawnwaker.)
    pub ability_discount: u8,
    /// Ascend (CR 702.131a): while it is on the battlefield, its controller
    /// gets the city's blessing once they control ten or more permanents.
    pub ascend: bool,
    /// "Spells you cast from anywhere other than your hand cost {n} less to
    /// cast" (Bilbo, Thief in the Night; CR 601.2f).
    pub outside_hand_discount: u8,
    /// "As this land enters, choose a basic land type ... This land is the
    /// chosen type" (Multiversal Passage; `Mark::ChosenColor` holds the
    /// type's index W U B R G: Plains Island Swamp Mountain Forest).
    pub chooses_land_type: bool,
    /// "As an additional cost to cast this spell, pay X life" (Fire
    /// Covenant): X is announced (CR 601.2b) and paid as life.
    pub x_life: bool,
    /// Miracle (CR 702.94): its cost, and the delayed ability k (its
    /// trigger `delayed_base + k`) "when you reveal this card this way, you
    /// may cast it by paying [cost]".
    pub miracle: Option<(ManaCost, u8)>,
    // ---- M7d (ENGINE.md 31) ----
    /// An opening-hand action (CR 103.6): "you may begin the game with it on
    /// the battlefield" (Gemstone Caverns).
    pub opening: Option<Opening>,
    /// "If a card would be put into an opponent's graveyard from anywhere,
    /// instead exile it with a <counter> on it" (Dauthi Voidwalker; CR
    /// 614.1a).
    pub void_exile: Option<Counter>,
    /// Reconfigure (CR 702.151): while attached to a creature, it isn't a
    /// creature (702.151b); it may equip while a creature (301.5c).
    pub reconfigure: bool,
    /// A Class (CR 716): its level (1 on the card, n on level n's face; 0 =
    /// not a Class), and the faces of levels 2, 3, ... in order (on the card
    /// and on each level face). A Class gaining a level turns that face up
    /// (the same object, as transforming).
    pub class_level: u8,
    pub levels: Vec<DefId>,
    /// "As this enters, look at an opponent's hand, then choose any card
    /// name" (Anointed Peacekeeper; `Mark::ChosenName`); "spells your
    /// opponents cast with the chosen name cost {n} more", "activated
    /// abilities of sources with the chosen name cost {m} more to activate
    /// unless they're mana abilities" (CR 601.2f, 602.2b).
    pub chooses_name: bool,
    pub named_spell_tax: u8,
    pub named_ability_tax: u8,
    /// "You may look at the top card of your library any time" (Bolas's
    /// Citadel): its controller knows their top card (`Game::look_tops`).
    pub look_top: bool,
    /// "You may play lands and cast spells from the top of your library. If
    /// you cast a spell this way, pay life equal to its mana value rather
    /// than pay its mana cost" (`CastFrom::LibraryTop`).
    pub play_top_life: bool,
    // ---- M8a (ENGINE.md 35) ----
    /// Companion (CR 702.139): its condition on its owner's starting deck.
    /// The card may be revealed from its owner's sideboard before the game
    /// (103.2b) and put into their hand once for {3} (116.2g).
    pub companion: Option<DeckCond>,
    /// Rules about casting spells this card adds (Edge of Eternities lane
    /// C, ENGINE.md 99.4).
    pub casting: CastingRules,
    /// "Each creature you control with toughness greater than its power
    /// assigns combat damage equal to its toughness rather than its power"
    /// (CR 510.1a; ENGINE.md 99.8).
    pub toughness_assigns_damage: bool,
    /// "... stations permanents using its toughness rather than its power"
    /// (CR 702.184c; ENGINE.md 99.8).
    pub toughness_stations: bool,
}

/// Rules about casting spells a card adds while it is on the battlefield
/// (or, for its own warp, wherever the card is): Edge of Eternities lane C
/// (ENGINE.md 99.4).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct CastingRules {
    /// Reductions of the cost of each spell its controller casts (CR
    /// 601.2f): "the second spell you cast each turn costs {2} less to
    /// cast"; "spells you cast have affinity for artifacts" (702.41a).
    pub spell_discounts: Vec<Discount>,
    /// "Each player can't cast more than one noncreature spell each turn."
    pub one_noncreature_spell: bool,
    /// "The first spell each player casts during each of their turns may
    /// be cast without paying its mana cost" (an alternative cost, CR
    /// 118.9).
    pub first_spell_free: bool,
    /// "You may cast this card from your graveyard using its warp ability"
    /// (CR 702.185a: warp's alternative cost from a graveyard).
    pub warp_from_graveyard: bool,
    /// Its own convoke (CR 702.51a): while it is a spell, its controller's
    /// untapped creatures may each pay for {1} or one mana of that
    /// creature's colour (`PayMana::convoke`; ENGINE.md 99.8).
    pub convoke: bool,
}

/// "If this card is in your opening hand [and you're not the starting
/// player], you may begin the game with it on the battlefield [with these
/// counters on it]. If you do, exile n cards from your hand." (CR 103.6)
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Opening {
    pub not_starting: bool,
    pub counters: Vec<(Counter, u8)>,
    pub exile: u8,
}

/// `CardDef::gy_play_k` when the permission grants nothing.
pub const NO_GRANT: u8 = u8::MAX;

/// A ward cost (CR 702.21a-b): mana, life, or a card to discard.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WardCost {
    Mana(ManaCost),
    Life(u8),
    Discard,
}

/// A cost reduction of a spell or an activated ability (CR 601.2f, 602.2b),
/// in generic mana.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Discount {
    /// n less per basic land type among lands you control (domain).
    PerBasicLandType(u8),
    /// n less per permanent you control matching the filter (affinity).
    PerControlled(Filter, u8),
    /// n less per differently named land you control (ENGINE.md 99.3).
    PerDifferentlyNamedLand(u8),
    /// n less if the condition holds for you as the cost is determined
    /// (CR 601.2f: "this spell costs {n} less to cast if ...").
    If(Cond, u8),
    /// n less if one of the spell's targets matches the filter ("this
    /// spell costs {1} less to cast if it targets a blocking creature":
    /// the targets are chosen before the cost is determined, CR 601.2c,
    /// 601.2f). A spell's own reduction only.
    IfTargets(Filter, u8),
}

impl CardDef {
    pub fn is(&self, t: u16) -> bool {
        self.types & t != 0
    }
    /// Does the card have an activated ability (CR 602; `Filter::
    /// HasActivatedAbility`)? Its printed ones: those of the permanent (mana
    /// abilities, the intrinsic ones of basic land types included, 305.6;
    /// loyalty abilities, 606; equip, reconfigure, Class levels), from hand
    /// (cycling, channel) and from a graveyard. Not special actions (plot,
    /// turning face up: CR 116.2b, 116.2k) nor latent abilities, which only an
    /// effect gives (`latent`).
    pub fn has_activated_ability(&self) -> bool {
        let own = self.activated.len() - (self.latent as usize).min(self.activated.len());
        self.activated[..own].iter().any(|a| a.special.is_none())
            || self.hand_abilities.iter().any(|a| a.special.is_none())
            || self.gy_abilities.iter().any(|a| a.special.is_none())
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
    /// The face the card may be cast or played with instead of its front: a
    /// split card's right half (CR 709.3), a modal double-faced card's back
    /// face (712.11b, 712.12), an adventurer card's Adventure (715.3).
    #[inline]
    pub fn other_face(&self) -> Option<DefId> {
        if let Some(h) = self.halves {
            Some(h.1)
        } else if self.modal {
            self.back
        } else {
            self.adventure
        }
    }
    /// The activated ability in `slot` (`Activated` or `Hand`).
    pub fn activated_def(&self, slot: AbilitySlot) -> &ActivatedDef {
        match slot {
            AbilitySlot::Activated(i) => &self.activated[i as usize],
            AbilitySlot::Hand(i) => &self.hand_abilities[i as usize],
            AbilitySlot::Graveyard(i) => &self.gy_abilities[i as usize],
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
    /// The interned "Equipment" subtype, if any card has it.
    equipment: Option<u16>,
    /// The finality counter (CR 122.1i), if any card names it.
    pub(crate) finality: Option<Counter>,
    /// The definition of a face-down permanent (CR 708.2a): a 2/2 creature
    /// with no text, no name, no subtypes and no mana cost; its one
    /// "ability" is the special action that turns it face up (708.7). The
    /// card it is waits in `Game::face_down_ids`.
    pub(crate) face_down: DefId,
    /// A route's cost and a card's own additional-cost option paid
    /// together (CR 601.2b, 601.2f-h): (card, route, option, the one cost).
    /// The route is the definition granting escape (Underworld Breach) or
    /// `RETRACE_ROUTE` (granted retrace).
    merged: Vec<(DefId, DefId, u8, Cost)>,
    /// A card of the table reads "dealt damage this turn"
    /// (`Filter::DealtDamageThisTurn`): the game records it
    /// (`Mark::DealtDamage`; EoE lane D, ENGINE.md 99.5).
    pub(crate) watch_damage: bool,
    /// A card of the table has `toughness_assigns_damage` or
    /// `toughness_stations`: combat damage and station look for it, and the
    /// game keeps permanents' last-known toughness (`Game::lki_toughness`;
    /// ENGINE.md 99.8).
    pub(crate) watch_toughness: bool,
}

/// `CardDb::merged_cost`'s route for granted retrace (CR 702.81a).
pub(crate) const RETRACE_ROUTE: DefId = DefId::MAX;

/// `CardDb::merged_cost`'s option for alternative cost k: `ALT_OPTION + k`
/// (retrace's land discard and the alternative cost's rest as one cost).
pub(crate) const ALT_OPTION: u8 = 0x80;

/// Two costs paid as one (a granted escape or retrace with a card's own
/// additional cost, CR 601.2b): `None` when the engine's single cost can't
/// say both (two different discards, two sacrifices; at most one of the
/// reservations `Game::keep_for` makes).
pub(crate) fn merge_costs(a: &Cost, b: &Cost) -> Option<Cost> {
    fn one<T: Clone>(x: &Option<T>, y: &Option<T>) -> Option<Option<T>> {
        match (x, y) {
            (Some(_), Some(_)) => None,
            _ => Some(x.clone().or_else(|| y.clone())),
        }
    }
    let discard_filter = match (a.discard > 0, b.discard > 0) {
        (true, true) if a.discard_filter != b.discard_filter => return None,
        (true, _) => a.discard_filter.clone(),
        (_, true) => b.discard_filter.clone(),
        _ => None,
    };
    let c = Cost {
        mana: a.mana.plus(&b.mana),
        tap: a.tap || b.tap,
        life: a.life.checked_add(b.life)?,
        sac_self: a.sac_self || b.sac_self,
        exile_self: a.exile_self || b.exile_self,
        discard_self: a.discard_self || b.discard_self,
        discard_hand: a.discard_hand || b.discard_hand,
        sacrifice: one(&a.sacrifice, &b.sacrifice)?,
        discard: a.discard + b.discard,
        discard_filter,
        exile_from_hand: one(&a.exile_from_hand, &b.exile_from_hand)?,
        bounce: one(&a.bounce, &b.bounce)?,
        exile_graveyard: a.exile_graveyard.checked_add(b.exile_graveyard)?,
        remove_counters: one(&a.remove_counters, &b.remove_counters)?,
        crew: if a.crew > 0 && b.crew > 0 { return None } else { a.crew + b.crew },
        saddle: a.saddle || b.saddle,
        tap_untapped: one(&a.tap_untapped, &b.tap_untapped)?,
        waterbend: a.waterbend || b.waterbend,
        exert: a.exert || b.exert,
    };
    let reservations = [c.sacrifice.is_some() || c.bounce.is_some(), c.discard > 0, c.exile_from_hand.is_some()];
    if reservations.iter().filter(|&&x| x).count() > 1 || c.sacrifice.is_some() && c.bounce.is_some() {
        return None;
    }
    Some(c)
}

impl CardDb {
    /// The cost of casting card `card` by `route` (a granter of escape, or
    /// `RETRACE_ROUTE`) with its additional-cost option `option`, if both
    /// can be paid as one cost.
    pub(crate) fn merged_cost(&self, card: DefId, route: DefId, option: u8) -> Option<&Cost> {
        self.merged.iter().find(|m| m.0 == card && m.1 == route && m.2 == option).map(|m| &m.3)
    }

    /// Lower every card; supported ones become definitions, the others are
    /// recorded with every feature that blocks them.
    pub fn from_cards(cards: &[card_ir::Card]) -> CardDb {
        let mut db = CardDb::default();
        let mut tokens: Vec<CardDef> = Vec::new();
        // The faces of each supported card (its back face, its Adventure).
        let mut faces: Vec<(DefId, Vec<CardDef>)> = Vec::new();
        // Double-faced tokens: (token index, the card whose faces hold its
        // back face, that face's index).
        let mut token_faces: Vec<(usize, DefId, DefId)> = Vec::new();
        let face_tag = crate::lower::effect_ext::TOKEN_FACE;
        for card in cards {
            let t0 = tokens.len();
            let result = lower::lower_card(card, &mut db.subtypes, &mut tokens);
            for (ti, t) in tokens.iter_mut().enumerate().skip(t0) {
                if let Some(k) = t.back.filter(|k| k & face_tag != 0) {
                    match result {
                        Ok(_) => token_faces.push((ti, db.defs.len() as DefId, k & !face_tag)),
                        Err(_) => t.back = None,
                    }
                }
            }
            match result {
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
        db.equipment = db.subtype_id("Equipment");
        // (Placed with the faces, below: after every card and token.)
        db.finality = db.subtype_id("counter:finality").map(Counter::Named);
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
            card.bestow = card.bestow.map(|k| base + k);
            card.prepare = card.prepare.map(|k| base + k);
            card.halves = card.halves.map(|(a, b)| (base + a, base + b));
            for k in card.levels.iter_mut() {
                *k += base;
            }
            let levels = card.levels.clone();
            for (k, mut f) in fs.into_iter().enumerate() {
                f.front = Some(id);
                if levels.contains(&(base + k as DefId)) {
                    f.levels = levels.clone();
                }
                if back == Some(base + k as DefId) {
                    f.back = Some(id); // a back face transforms into its front
                }
                // A double-faced token's back face (its token is its front).
                if let Some(&(ti, _, _)) = token_faces.iter().find(|&&(_, c, fk)| c == id && fk == k as DefId) {
                    let t = db.token_base + ti as DefId;
                    f.front = Some(t);
                    f.back = Some(t);
                    db.defs[t as usize].back = Some(base + k as DefId);
                }
                db.defs.push(f);
            }
        }
        db.face_down = db.defs.len() as DefId;
        db.defs.push(face_down_def());
        // A granted escape (Underworld Breach) or retrace (Wrenn and Six,
        // Six) with the card's own additional costs: each option and the
        // route's cost as one cost (CR 601.2b, 601.2f-h; Bitter Triumph).
        let granters: Vec<(DefId, Cost)> =
            db.defs.iter().enumerate().filter_map(|(i, d)| d.grants_escape.clone().map(|c| (i as DefId, c))).collect();
        let retrace = Cost { discard: 1, discard_filter: Some(Filter::Type(types::LAND)), ..Default::default() };
        let mut merged = Vec::new();
        for (i, d) in db.defs.iter().enumerate() {
            if d.is(types::LAND) {
                continue;
            }
            // Retrace with an alternative cost (CR 702.81a: retrace adds a
            // cost; 118.9).
            for (k, a) in d.alt_costs.iter().enumerate() {
                if let Some(c) = merge_costs(&retrace, &a.cost) {
                    merged.push((i as DefId, RETRACE_ROUTE, ALT_OPTION + k as u8, c));
                }
            }
            if d.additional.is_empty() {
                continue;
            }
            for (k, o) in d.additional.iter().enumerate() {
                for (g, gc) in &granters {
                    if let Some(c) = merge_costs(gc, o) {
                        merged.push((i as DefId, *g, k as u8, c));
                    }
                }
                if let Some(c) = merge_costs(&retrace, o) {
                    merged.push((i as DefId, RETRACE_ROUTE, k as u8, c));
                }
            }
        }
        db.merged = merged;
        db.watch_damage = db.defs.iter().any(crate::resolve_eoe_d::reads_dealt_damage);
        db.watch_toughness = db.defs.iter().any(|d| d.toughness_assigns_damage || d.toughness_stations);
        db
    }

    /// The definition of face-down permanents (CR 708.2a).
    pub fn face_down_def(&self) -> DefId {
        self.face_down
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

    /// The card table of constructed play (ENGINE.md 54): the cube's cards
    /// (the same definitions at the same ids as [`cube`](Self::cube)) and
    /// then the cards of `data/cards/legacy.ir.json` (cards outside the
    /// cube a constructed deck plays: the Izzet Delver mirror's Murktide
    /// Regent and Prismari Charm), then their tokens and faces. A separate
    /// table: nothing of the cube (its deck generators, pools, team draft,
    /// fuzz pool, digests) ever sees the legacy cards. Every legacy card
    /// must be supported (a loud failure otherwise).
    pub fn constructed() -> &'static CardDb {
        static CONSTRUCTED: OnceLock<&'static CardDb> = OnceLock::new();
        CONSTRUCTED.get_or_init(|| {
            let mut cards = load_cube_document().cards;
            let legacy = load_legacy_document().cards;
            let names: Vec<String> = legacy.iter().map(|c| c.name.clone()).collect();
            cards.extend(legacy);
            let db = CardDb::from_cards(&cards);
            for n in &names {
                db.expect_id(n).unwrap_or_else(|e| panic!("legacy card IR: {e}"));
            }
            db.leak()
        })
    }

    /// The card table of Edge of Eternities sealed (ENGINE.md 99): the cards
    /// an EoE play booster can hold (Edge of Eternities, Stellar Sights, the
    /// EoE Special Guests) and the five basic lands, from
    /// `data/cards/eoe.ir.json`, then their tokens and faces. A separate
    /// table, like [`constructed`](Self::constructed): nothing of the cube
    /// (its deck generators, pools, team draft, fuzz pool, digests) ever sees
    /// it, and it holds no cube definition (the cards the set shares with the
    /// cube are lowered from the set's own export of the same parser).
    /// Unsupported cards are allowed while the port is under way: they stay
    /// out of `by_name`, and the support report (`tests/support.rs`) lists
    /// their blockers.
    pub fn eoe() -> &'static CardDb {
        static EOE: OnceLock<&'static CardDb> = OnceLock::new();
        EOE.get_or_init(|| CardDb::from_document(&load_eoe_document()).leak())
    }

    pub fn id(&self, name: &str) -> Option<DefId> {
        self.by_name.get(name).copied()
    }

    /// Does card `def`, outside the game or in a library (its own
    /// characteristics, CR 712.8a, 715.4), match `f`? `None` for a filter
    /// that needs a game (a controller, combat, counters): the lowering
    /// accepts only filters this answers in a deck condition
    /// (`deck_filter_ok`).
    pub fn def_matches(&self, f: &Filter, def: DefId) -> Option<bool> {
        let d = self.def(def);
        Some(match f {
            Filter::Any => true,
            Filter::Type(bits) => d.types & bits != 0,
            Filter::Supertype(bits) => d.supertypes & bits != 0,
            Filter::Color(m) => d.colors & m != 0,
            Filter::Colorless => d.colors == 0,
            Filter::ManaValueAtMost(n) => d.mana_value() <= *n,
            Filter::ManaValueAtLeast(n) => d.mana_value() >= *n,
            Filter::Named(n) => d.name == *n,
            Filter::HasKeyword(k) => d.keywords.has(*k),
            Filter::HasActivatedAbility => d.has_activated_ability(),
            Filter::And(a, b) => self.def_matches(a, def)? && self.def_matches(b, def)?,
            Filter::Or(v) => {
                let mut any = false;
                for x in v {
                    any |= self.def_matches(x, def)?;
                }
                any
            }
            Filter::Not(x) => !self.def_matches(x, def)?,
            _ => return None,
        })
    }

    /// Does the starting deck `deck` fulfill a companion's condition (CR
    /// 702.139a-b)? Exact over the deck's cards (`def_matches`).
    pub fn deck_satisfies(&self, cond: &DeckCond, deck: &[DefId]) -> bool {
        let m = |f: &Filter, id: DefId| self.def_matches(f, id) == Some(true);
        match cond {
            DeckCond::Each { cards, must } => deck.iter().all(|&id| !m(cards, id) || m(must, id)),
            DeckCond::DifferentNames { cards } => {
                let mut names: Vec<&str> = deck.iter().filter(|&&id| m(cards, id)).map(|&id| self.def(id).name.as_str()).collect();
                let n = names.len();
                names.sort_unstable();
                names.dedup();
                names.len() == n
            }
        }
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
            AbilitySlot::Activated(_) | AbilitySlot::Hand(_) | AbilitySlot::Graveyard(_) => match &d.activated_def(r.slot).effect {
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

    /// Every interned subtype (and counter name), by id.
    pub fn subtype_names(&self) -> &[String] {
        &self.subtypes
    }

    /// The "Equipment" subtype's id.
    pub fn equipment_subtype(&self) -> Option<u16> {
        self.equipment
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

/// The checked-in legacy IR (cards outside the cube for constructed decks,
/// ENGINE.md 54), compiled in; encoded with the cube's schema (one
/// `card_ir` crate reads both: mtg-bot `cards export-ir --pool cube legacy`).
pub const LEGACY_IR: &str = include_str!("../../data/cards/legacy.ir.json");

pub fn load_legacy_document() -> card_ir::Document {
    card_ir::load_document(LEGACY_IR).unwrap_or_else(|e| panic!("{e}"))
}

/// The checked-in Edge of Eternities IR (ENGINE.md 99; the set's cards and
/// the basic lands), compiled in; encoded with the cube's schema (mtg-bot
/// `cards export-ir --pool cube legacy eoe`, then
/// `tools/codegen/filter_eoe_ir.py` drops the pool's Duskmourn cards).
pub const EOE_IR: &str = include_str!("../../data/cards/eoe.ir.json");

pub fn load_eoe_document() -> card_ir::Document {
    card_ir::load_document(EOE_IR).unwrap_or_else(|e| panic!("{e}"))
}

/// A face-down permanent's characteristics (CR 708.2a): a 2/2 creature, no
/// name, colour, mana cost, subtypes or abilities. Its one activated entry is
/// the special action of turning it face up (CR 708.7, 701.40b; its cost is
/// the card's mana cost, `Game::face_down_cost`).
fn face_down_def() -> CardDef {
    CardDef {
        name: String::new(),
        types: types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        activated: vec![ActivatedDef {
            cost: Cost::default(),
            effect: ActEffect::Stack(AbilityDef::default()),
            sorcery: false,
            instant_only: false,
            exhaust: false,
            only_if: None,
            your_turn: false,
            loyalty: None,
            discounts: Vec::new(),
            boast: false,
            x: false,
            special: Some(Special::TurnFaceUp),
        }],
        ..CardDef::default()
    }
}
