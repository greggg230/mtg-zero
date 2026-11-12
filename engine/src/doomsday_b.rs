//! Lane B of the Legacy Doomsday deck (ENGINE.md 124.2): Misdirection,
//! Flusterstorm, Cavern of Souls. Named after the rules actions, each citing
//! its rule; never after a card.
//!
//! * "Change the target of target spell with a single target" (CR 115.7a,
//!   115.9a): [`DoomsdayB::ChangeTheTarget`] and `Filter::SingleTarget`.
//! * "As this land enters, choose a creature type" (CR 614.12, 205.3m):
//!   every type of [`CREATURE_TYPES_CR`] by number (`Task::ChooseCreatureType`,
//!   `ask_number`), kept as `Mark::ChosenCreatureType`.
//! * "Spend this mana only to cast a creature spell of the chosen type, and
//!   that spell can't be countered" (CR 106.6): `SPEND_CHOSEN_CREATURE_TYPE`
//!   on the mana ability; the mana it makes carries its source's type in its
//!   restriction ([`with_type`]: any number of types at once, a pool kind
//!   each), and a payment for a creature spell allows the types it has
//!   ([`Allow::types`], `Game::creature_type_allow`); the rider is Delighted
//!   Halfling's.

use crate::db::{types, Spend, SPEND_CHOSEN_CREATURE_TYPE};
use crate::decision::{Choice, DecisionKind, NumberAnswer, NumberStep};
use crate::flow::Task;
use crate::game::{Game, Mark, StackKind};
use crate::ids::{CardIdx, Target, Zone};
use crate::resolve::{Flow, Frame};

/// The creature types of CR 205.3m (the Comprehensive Rules of 2026-11-06:
/// "Time Lord" and the 366 one-word types), sorted by bytes, apostrophes
/// ASCII: what "choose a creature type" chooses from (CR 205.3e: one existing
/// creature type; Cavern of Souls' ruling: no card type). The index is the
/// choice's number and the encoding's (`perm::CHOSEN_CREATURE_TYPE`).
pub const CREATURE_TYPES_CR: [&str; 367] = [
    "Advisor", "Aenar", "Aetherborn", "Alien", "Ally", "Andorian", "Android", "Angel", "Antelope",
    "Ape", "Archer", "Archon", "Armadillo", "Army", "Artificer", "Assassin", "Assembly-Worker",
    "Astartes", "Atog", "Aurochs", "Avatar", "Azra", "Badger", "Bajoran", "Balloon", "Barbarian",
    "Bard", "Basilisk", "Bat", "Bear", "Beast", "Beaver", "Beeble", "Beholder", "Berserker",
    "Betazoid", "Bird", "Bison", "Blinkmoth", "Boar", "Bolian", "Borg", "Brikar", "Bringer",
    "Brushwagg", "C'tan", "Caitian", "Camarid", "Camel", "Capybara", "Cardassian", "Caribou",
    "Carrier", "Cat", "Centaur", "Child", "Chimera", "Citizen", "Cleric", "Clown", "Cockatrice",
    "Construct", "Coward", "Coyote", "Crab", "Crocodile", "Custodes", "Cyberman", "Cyclops",
    "Dalek", "Dauthi", "Demigod", "Demon", "Denobulan", "Deserter", "Detective", "Devil",
    "Dinosaur", "Djinn", "Doctor", "Dog", "Dragon", "Drake", "Dreadnought", "Drix", "Drone",
    "Druid", "Dryad", "Dwarf", "Echidna", "Edosian", "Efreet", "Egg", "El-Aurian", "Elder",
    "Eldrazi", "Elemental", "Elephant", "Elf", "Elk", "Employee", "Eternal", "Excalbian", "Eye",
    "Faerie", "Ferengi", "Ferret", "Fish", "Flagbearer", "Fox", "Fractal", "Frog", "Fungus",
    "Gamer", "Gamma", "Gargoyle", "Germ", "Giant", "Giraffe", "Gith", "Glimmer", "Gnoll", "Gnome",
    "Goat", "Goblin", "God", "Golem", "Gorgon", "Gorn", "Graveborn", "Gremlin", "Griffin", "Guest",
    "Hag", "Halfling", "Hamster", "Harpy", "Hedgehog", "Hellion", "Hero", "Hippo", "Hippogriff",
    "Hirogen", "Homarid", "Homunculus", "Horror", "Horse", "Human", "Hydra", "Hyena", "Illusion",
    "Illyrian", "Imp", "Incarnation", "Inhuman", "Inkling", "Inquisitor", "Insect", "Jackal",
    "Jellyfish", "Jem'Hadar", "Juggernaut", "Kangaroo", "Kavu", "Kelpien", "Kirin", "Kithkin",
    "Klingon", "Knight", "Kobold", "Kor", "Kraken", "Kree", "Kwejian", "Lamia", "Lammasu",
    "Lanthanite", "Leech", "Lemur", "Leviathan", "Lhurgoyf", "Licid", "Lizard", "Llama", "Lobster",
    "Manticore", "Masticore", "Medusan", "Melkotian", "Mercenary", "Merfolk", "Metathran", "Minion",
    "Minotaur", "Mite", "Mole", "Monger", "Mongoose", "Monk", "Monkey", "Moogle", "Moonfolk",
    "Mount", "Mouse", "Mutant", "Myr", "Mystic", "Na'kuhl", "Nautilus", "Necron", "Nephilim",
    "Nightmare", "Nightstalker", "Ninja", "Noble", "Noggle", "Nomad", "Nymph", "Ocampa", "Octopus",
    "Officer", "Ogre", "Ooze", "Orb", "Orc", "Orgg", "Orion", "Otter", "Ouphe", "Ox", "Oyster",
    "Pangolin", "Peasant", "Pegasus", "Pentavite", "Performer", "Pest", "Phelddagrif", "Phoenix",
    "Phyrexian", "Pilot", "Pincher", "Pirate", "Plant", "Platypus", "Porcupine", "Possum",
    "Praetor", "Primarch", "Prism", "Processor", "Q", "Qu", "Rabbit", "Raccoon", "Ranger", "Rat",
    "Rebel", "Reflection", "Rhino", "Rigger", "Robot", "Rogue", "Romulan", "Sable", "Salamander",
    "Samurai", "Sand", "Saproling", "Satyr", "Scarecrow", "Scientist", "Scion", "Scorpion", "Scout",
    "Sculpture", "Seal", "Serf", "Serpent", "Servo", "Shade", "Shaman", "Shapeshifter", "Shark",
    "Sheep", "Shi'ar", "Siren", "Skeleton", "Skrull", "Skunk", "Slith", "Sliver", "Sloth", "Slug",
    "Snail", "Snake", "Soldier", "Soltari", "Sorcerer", "Spawn", "Specter", "Spellshaper", "Sphinx",
    "Spider", "Spike", "Spirit", "Splinter", "Sponge", "Spy", "Squid", "Squirrel", "Starfish",
    "Surrakar", "Survivor", "Symbiote", "Synth", "Talaxian", "Talosian", "Tamarian", "Tellarite",
    "Tentacle", "Tetravite", "Thalakos", "Tholian", "Thopter", "Thrull", "Tiefling", "Time Lord",
    "Tosk", "Toy", "Treefolk", "Tribble", "Trill", "Trilobite", "Triskelavite", "Troll", "Turtle",
    "Tyranid", "Unicorn", "Utrom", "Vampire", "Varmint", "Vau-N'Akat", "Vedalken", "Villain",
    "Volver", "Vorta", "Vulcan", "Wall", "Walrus", "Warlock", "Warrior", "Weasel", "Weird",
    "Werewolf", "Whale", "Wizard", "Wolf", "Wolverine", "Wombat", "Worm", "Wraith", "Wurm", "Xindi",
    "Yeti", "Zombie", "Zubera",
];

/// Where the chosen creature type of mana restricted to it sits in its
/// `Spend` (with `SPEND_CHOSEN_CREATURE_TYPE`): bits 16-24 hold its
/// `CREATURE_TYPES_CR` index + 1, 0 when its source chose no type (then no
/// spell is of the chosen type).
pub const TYPE_SHIFT: u32 = 16;
pub const TYPE_FIELD: Spend = 0x1FF << TYPE_SHIFT;
const _: () = assert!(CREATURE_TYPES_CR.len() < 0x1FF && TYPE_FIELD & 0xFFFF == 0);

/// The chosen creature type of mana restricted to one (`only` has
/// `SPEND_CHOSEN_CREATURE_TYPE`; `None`: its source chose none).
pub fn type_of(only: Spend) -> Option<u16> {
    let v = (only & TYPE_FIELD) >> TYPE_SHIFT;
    (v > 0).then(|| (v - 1) as u16)
}

/// `only` restricted to creature spells of type `t` (`None`: of no type).
pub fn with_type(only: Spend, t: Option<u16>) -> Spend {
    (only & !TYPE_FIELD) | SPEND_CHOSEN_CREATURE_TYPE | t.map_or(0, |t| (t as Spend + 1) << TYPE_SHIFT)
}

/// A set of creature types (bit t: `CREATURE_TYPES_CR[t]`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct CreatureTypes(pub [u64; 6]);

impl CreatureTypes {
    pub fn insert(&mut self, t: u16) {
        self.0[t as usize / 64] |= 1 << (t % 64);
    }
    pub fn contains(&self, t: u16) -> bool {
        self.0[t as usize / 64] >> (t % 64) & 1 != 0
    }
    pub fn is_empty(&self) -> bool {
        self.0 == [0; 6]
    }
}

/// What a payment satisfies of spending restrictions (CR 106.6; `pay::Keep`):
/// the purposes (`SPEND_*` bits), and, paying for a creature spell, the
/// creature types it has among those some mana is restricted to
/// (ENGINE.md 124.2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Allow {
    pub bits: Spend,
    pub types: CreatureTypes,
}

impl From<Spend> for Allow {
    fn from(bits: Spend) -> Allow {
        Allow { bits, types: CreatureTypes::default() }
    }
}

impl std::ops::BitOr<Spend> for Allow {
    type Output = Allow;
    fn bitor(self, b: Spend) -> Allow {
        Allow { bits: self.bits | b, ..self }
    }
}

impl Allow {
    /// Nothing allowed beyond unrestricted mana.
    pub fn is_empty(&self) -> bool {
        self.bits == 0 && self.types.is_empty()
    }
}

/// Can a card make mana restricted to creature spells of a chosen type (CR 106.6; Cavern of Souls)?
/// Layout v6 has no float for such mana floating (ENGINE.md 124.2); layout v7's block encodes it, and
/// the layout v6 game `mtg_eoe` refuses a card table holding such a card (`az::needs_layout_v7`).
pub fn makes_chosen_type_mana(d: &crate::db::CardDef) -> bool {
    d.activated.iter().chain(d.hand_abilities.iter()).filter_map(|a| a.mana()).any(|m| m.only & SPEND_CHOSEN_CREATURE_TYPE != 0)
}

/// The refusal of a layout v6 game with such a card (`makes_chosen_type_mana`).
pub fn chosen_type_mana_needs_v7(name: &str) -> String {
    let owner = if name.ends_with('s') { format!("{name}'") } else { format!("{name}'s") };
    format!("{owner} floating mana needs layout v7 (ENGINE.md 124.2)")
}

/// The printing facts lane B adds (`CardDef::dd_b`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct DdBDef {
    /// "As this enters, choose a creature type" (CR 614.12, 205.3m).
    pub chooses_creature_type: bool,
}

/// Lane B's instructions (`Ext::DoomsdayB`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum DoomsdayB {
    /// "Change the target of target spell" (CR 115.7a) for the spell of
    /// target requirement `tgt` (a spell with a single target, CR 115.9a):
    /// its controller (this ability's) changes that spell's one target to
    /// another target legal for it, from that spell's point of view (its
    /// controller and its own requirement; never the spell itself, 115.5;
    /// this spell, still on the stack as it resolves, is a candidate). With
    /// no other legal target the target is unchanged (115.7a), even if
    /// illegal; otherwise the change is not optional. The new target becomes
    /// its target then (ward and other "becomes the target" abilities see
    /// it). Choice: `NewTarget`, the candidates by range (paged), no `PASS`.
    ChangeTheTarget { tgt: u8 },
}

impl DoomsdayB {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            DoomsdayB::ChangeTheTarget { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            DoomsdayB::ChangeTheTarget { .. } => None,
        }
    }
}

impl Game {
    pub(crate) fn exec_doomsday_b(&mut self, f: &mut Frame, op: &DoomsdayB) -> Flow {
        match op {
            DoomsdayB::ChangeTheTarget { tgt } => self.change_the_target(f, *tgt),
        }
    }

    /// The stack item of the spell `t` names (a spell on the stack).
    fn spell_item_id(&self, t: Target) -> Option<u32> {
        match t {
            Target::Obj(o) if self.live(o) && self.objs[o.idx.i()].zone == Zone::Stack => {
                self.stack.iter().find(|s| s.kind == StackKind::Spell { card: o.idx }).map(|s| s.id)
            }
            _ => None,
        }
    }

    /// The targets other than its own that are legal for the single target
    /// of stack item `id` (CR 115.7a; 115.3, 115.5, 601.2c's total): its
    /// requirement's, from its controller and source.
    pub(crate) fn other_legal_targets(&self, id: u32) -> Vec<Target> {
        let Some(pos) = self.item_index(id) else { return Vec::new() };
        let item = &self.stack[pos];
        if item.targets.len() != 1 {
            return Vec::new();
        }
        let Some(ar) = self.item_ability(item) else { return Vec::new() };
        let reqs = crate::resolve_tail3::item_reqs(self.db, item, ar);
        let (req, cur) = item.targets[0];
        let Some(&tr) = reqs.get(req as usize) else { return Vec::new() };
        let ctx = crate::cast::Ctx { controller: item.controller, source: Some(self.item_source(item)) };
        let mut cands = self.target_candidates(tr, &ctx, &[]);
        if let Some(n) = tr.total_mv {
            self.retain_within_total_mv(n, &[], &mut cands);
        }
        cands.retain(|&t| t != cur);
        cands
    }

    fn change_the_target(&mut self, f: &mut Frame, tgt: u8) -> Flow {
        let Some(id) = self.targets_of(f, tgt).into_iter().find_map(|t| self.spell_item_id(t)) else {
            return Flow::Next;
        };
        match self.answer.take() {
            Some(Choice::Target(t)) => {
                let pos = self.item_index(id).expect("the spell whose target changes");
                self.note(|g| format!("its target becomes {}", g.describe_target(t)));
                self.stack[pos].targets[0].1 = t;
                self.became_target(id, t);
                Flow::Next
            }
            Some(other) => unreachable!("new target answer {other:?}"),
            None => {
                let cands = self.other_legal_targets(id);
                if cands.is_empty() {
                    // No other legal target: unchanged (CR 115.7a).
                    self.note(|_| "no other legal target: the target is unchanged".to_string());
                    return Flow::Next;
                }
                let items: Vec<Choice> = cands.into_iter().map(Choice::Target).collect();
                self.ask_objects(Task::Run(f.clone()), f.page, f.controller, DecisionKind::NewTarget, &items, Vec::new());
                Flow::Suspend
            }
        }
    }

    /// Can spell `c` not be countered (CR 101.2: "this spell can't be
    /// countered", mana spent on it said so, an effect says so)?
    pub fn spell_cant_be_countered(&self, c: CardIdx) -> bool {
        self.spell_uncounterable(c)
    }

    /// Is `c` a spell with a single target (CR 115.9a: one target chosen as
    /// it was put on the stack, each instance counted, as changed since;
    /// whether it is legal now doesn't matter)?
    pub(crate) fn spell_with_single_target(&self, c: crate::ids::CardIdx) -> bool {
        self.stack.iter().find(|s| s.kind == StackKind::Spell { card: c }).is_some_and(|s| s.targets.len() == 1)
    }
}

impl Game {
    /// The creature type `c` chose as it entered (`CREATURE_TYPES_CR`
    /// index; CR 614.12), if any.
    pub fn chosen_creature_type(&self, c: CardIdx) -> Option<u16> {
        if self.marks.is_empty() {
            return None;
        }
        let r = self.obj_ref(c);
        self.marks.iter().find_map(|&(o, m)| match m {
            Mark::ChosenCreatureType(t) if o == r => Some(t),
            _ => None,
        })
    }

    /// `c` chose creature type `t` (a `CREATURE_TYPES_CR` index; CR 614.12).
    pub(crate) fn set_chosen_creature_type(&mut self, c: CardIdx, t: u16) {
        assert!((t as usize) < CREATURE_TYPES_CR.len(), "creature type index {t}");
        let r = self.obj_ref(c);
        self.marks.retain(|&(o, m)| !(o == r && matches!(m, Mark::ChosenCreatureType(_))));
        self.marks.push((r, Mark::ChosenCreatureType(t)));
        self.note(|g| format!("{} chose the creature type {}", g.name(c), CREATURE_TYPES_CR[t as usize]));
    }

    /// The restriction of the mana source `c`'s ability with purposes `only`
    /// makes (CR 106.6): "a creature spell of the chosen type" gets the type
    /// `c` chose, or none (no spell is then of the chosen type).
    pub(crate) fn spend_of_source(&self, c: CardIdx, only: Spend) -> Spend {
        if only & SPEND_CHOSEN_CREATURE_TYPE == 0 {
            return only;
        }
        with_type(only, self.chosen_creature_type(c))
    }

    /// The creature types casting `card` satisfies (CR 106.6): those it has
    /// (a creature spell) among the types some mana could be restricted to
    /// now (a permanent's chosen type, a pool's mana; the others matter to
    /// no payment). Only in tables whose mana has such a restriction.
    pub(crate) fn creature_type_allow(&self, card: CardIdx) -> CreatureTypes {
        let mut a = CreatureTypes::default();
        if self.db.spend_kinds & SPEND_CHOSEN_CREATURE_TYPE == 0 || !self.is_type(card, types::CREATURE) {
            return a;
        }
        let marked = self.marks.iter().filter_map(|&(_, m)| match m {
            Mark::ChosenCreatureType(t) => Some(t),
            _ => None,
        });
        let pooled = self.pool_restr.iter().filter(|k| k.2 & SPEND_CHOSEN_CREATURE_TYPE != 0).filter_map(|k| type_of(k.2));
        for t in marked.chain(pooled) {
            if !a.contains(t) && self.has_creature_type(card, t) {
                a.insert(t);
            }
        }
        a
    }

    /// Does `c` have creature type `t` (CR 205.3m; its subtypes as they are)?
    /// (A type no card of the table has is no object's: the table's
    /// subtypes are interned by name, Scryfall's apostrophe or ours.)
    pub fn has_creature_type(&self, c: CardIdx, t: u16) -> bool {
        let name = CREATURE_TYPES_CR[t as usize];
        let id = self.db.subtype_id(name).or_else(|| self.db.subtype_id(&name.replace('\'', "\u{2019}")));
        id.is_some_and(|s| self.has_subtype(c, s))
    }

    /// "As this enters, choose a creature type" (CR 614.12, 205.3e): one of
    /// the 367 of CR 205.3m, by its `CREATURE_TYPES_CR` index (`ask_number`:
    /// `NUMBER` + k for the first 31, then base-32 digits; ENGINE.md 100.1).
    /// Choice: `ChooseMode`, by `card`'s controller.
    pub(crate) fn choose_creature_type(&mut self, card: CardIdx, step: NumberStep) {
        let o = &self.objs[card.i()];
        if o.zone != Zone::Battlefield {
            return;
        }
        let p = o.controller;
        let hi = CREATURE_TYPES_CR.len() as u64 - 1;
        match self.answer.take() {
            Some(Choice::Number(k)) => match step.answer(0, hi, k) {
                NumberAnswer::Next(s) => self.agenda.push(Task::ChooseCreatureType { card, step: s }),
                NumberAnswer::Value(v) => self.set_chosen_creature_type(card, v as u16),
            },
            Some(other) => unreachable!("creature type answer {other:?}"),
            None => self.ask_number(Task::ChooseCreatureType { card, step }, step, p, DecisionKind::ChooseMode, 0, hi),
        }
    }
}
