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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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
}

impl AbilityDef {
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

/// Events triggered abilities listen for (ENGINE.md 8).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum EventKind {
    /// A permanent entered the battlefield (CR 603.6a).
    Enters,
    /// A creature was put into a graveyard from the battlefield (CR 700.4).
    Dies,
}

/// Which objects a trigger watches.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TrigFilter {
    /// The event's subject is the trigger's own source ("When this ...").
    SelfObj,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TriggerDef {
    pub event: EventKind,
    pub filter: TrigFilter,
    pub ability: AbilityDef,
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
    /// Discard n cards of your choice.
    pub discard: u8,
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
#[derive(Clone, PartialEq, Eq, Debug)]
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
}

impl CardDef {
    pub fn is(&self, t: u16) -> bool {
        self.types & t != 0
    }
    pub fn is_permanent_card(&self) -> bool {
        self.types & types::PERMANENT != 0
    }
    pub fn mana_value(&self) -> u8 {
        self.cost.mana_value()
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
        for card in cards {
            match lower::lower_card(card, &mut db.subtypes, &mut tokens) {
                Ok(def) => {
                    let id = db.defs.len() as DefId;
                    assert!(
                        db.by_name.insert(card.name.clone(), id).is_none(),
                        "duplicate card name {}",
                        card.name
                    );
                    db.defs.push(def);
                }
                Err(blockers) => {
                    db.unsupported.insert(card.name.clone(), blockers);
                }
            }
        }
        // Tokens the cards create (CR 111), interned during lowering.
        db.token_base = db.defs.len() as DefId;
        db.defs.extend(tokens);
        db
    }

    /// The definition of token `t` (an index of `Op::CreateToken`).
    pub fn token_def(&self, t: u16) -> DefId {
        self.token_base + t
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
