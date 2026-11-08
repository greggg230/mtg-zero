//! Edge of Eternities, W1 (ENGINE.md 99.11): the ops the cards whose card IR
//! mtg-bot's parser fixed need. Named after the rules actions, each citing
//! its rule; never after a card.

use crate::db::Filter;
use crate::game::{Game, Pos};
use crate::ids::{Target, Zone};
use crate::program::On;
use crate::resolve::{Flow, Frame};

/// The creature types (CR 205.3m), sorted: what "all creature types" (CR
/// 205.3m; Mutavault's animation) is. The list as Scryfall's creature-type
/// catalog gives the rule's (fetched 2026-11-08, 377 types).
pub const CREATURE_TYPES: &[&str] = &[
    "Advisor", "Aenar", "Aetherborn", "Alien", "Ally", "Andorian", "Android", "Angel", "Antelope",
    "Ape", "Archer", "Archon", "Armadillo", "Army", "Artificer", "Assassin", "Assembly-Worker",
    "Astartes", "Atog", "Aurochs", "Automaton", "Avatar", "Azra", "Badger", "Bajoran", "Balloon",
    "Barbarian", "Bard", "Basilisk", "Bat", "Bear", "Beast", "Beaver", "Beeble", "Beholder",
    "Berserker", "Betazoid", "Bird", "Bison", "Blinkmoth", "Boar", "Bolian", "Borg", "Brainiac",
    "Brikar", "Bringer", "Brushwagg", "C'tan", "Caitian", "Camarid", "Camel", "Capybara",
    "Cardassian", "Caribou", "Carrier", "Cat", "Centaur", "Chicken", "Child", "Chimera", "Citizen",
    "Cleric", "Clown", "Cockatrice", "Construct", "Coward", "Coyote", "Crab", "Crocodile",
    "Custodes", "Cyberman", "Cyclops", "Dalek", "Dauthi", "Demigod", "Demon", "Denobulan",
    "Deserter", "Detective", "Devil", "Dinosaur", "Djinn", "Doctor", "Dog", "Dragon", "Drake",
    "Dreadnought", "Drix", "Drone", "Druid", "Dryad", "Dwarf", "Echidna", "Edosian", "Efreet",
    "Egg", "El-Aurian", "Elder", "Eldrazi", "Elemental", "Elephant", "Elf", "Elk", "Employee",
    "Eternal", "Excalbian", "Eye", "Faerie", "Ferengi", "Ferret", "Fish", "Flagbearer", "Fox",
    "Fractal", "Frog", "Fungus", "Gamer", "Gamma", "Gargoyle", "Germ", "Giant", "Giraffe", "Gith",
    "Glimmer", "Gnoll", "Gnome", "Goat", "Goblin", "God", "Golem", "Gorgon", "Gorn", "Graveborn",
    "Gremlin", "Griffin", "Guest", "Hag", "Halfling", "Hamster", "Harpy", "Head", "Hedgehog",
    "Hellion", "Hero", "Hippo", "Hippogriff", "Hirogen", "Homarid", "Homunculus", "Horror",
    "Horse", "Human", "Hydra", "Hyena", "Illusion", "Illyrian", "Imp", "Incarnation", "Inhuman",
    "Inkling", "Inquisitor", "Insect", "Jackal", "Jellyfish", "Jem'Hadar", "Juggernaut",
    "Kangaroo", "Kavu", "Kelpien", "Kirin", "Kithkin", "Klingon", "Knight", "Kobold", "Kor",
    "Kraken", "Kree", "Kwejian", "Lamia", "Lammasu", "Lanthanite", "Leech", "Lemur", "Leviathan",
    "Lhurgoyf", "Licid", "Lizard", "Llama", "Lobster", "Manticore", "Masticore", "Medusan",
    "Melkotian", "Mercenary", "Merfolk", "Metathran", "Mindless-One", "Minion", "Minotaur", "Mite",
    "Mole", "Monger", "Mongoose", "Monk", "Monkey", "Moogle", "Moonfolk", "Mount", "Mouse",
    "Mutant", "Myr", "Mystic", "Na'kuhl", "Naga", "Nautilus", "Necron", "Nephilim", "Nightmare",
    "Nightstalker", "Ninja", "Noble", "Noggle", "Nomad", "Nymph", "Ocampa", "Octopus", "Officer",
    "Ogre", "Ooze", "Orb", "Orc", "Orgg", "Orion", "Otter", "Ouphe", "Ox", "Oyster", "Pangolin",
    "Peasant", "Pegasus", "Pentavite", "Performer", "Pest", "Phelddagrif", "Phoenix", "Phyrexian",
    "Pilot", "Pincher", "Pirate", "Plant", "Platypus", "Porcupine", "Possum", "Praetor",
    "Primarch", "Prism", "Processor", "Q", "Qu", "Rabbit", "Raccoon", "Ranger", "Rat", "Rebel",
    "Reflection", "Reveler", "Rhino", "Rigger", "Robot", "Rogue", "Romulan", "Rukh", "Sable",
    "Salamander", "Samurai", "Sand", "Saproling", "Satyr", "Scarecrow", "Scientist", "Scion",
    "Scorpion", "Scout", "Sculpture", "Seal", "Serf", "Serpent", "Servo", "Shade", "Shaman",
    "Shapeshifter", "Shark", "Sheep", "Shi'ar", "Siren", "Skeleton", "Skrull", "Skunk", "Slith",
    "Sliver", "Sloth", "Slug", "Snail", "Snake", "Soldier", "Soltari", "Sorcerer", "Spawn",
    "Specter", "Spellshaper", "Sphinx", "Spider", "Spike", "Spirit", "Splinter", "Sponge", "Spy",
    "Squid", "Squirrel", "Starfish", "Surrakar", "Survivor", "Symbiote", "Synth", "Talaxian",
    "Talosian", "Tamarian", "Teddy", "Tellarite", "Tentacle", "Tetravite", "Thalakos", "Tholian",
    "Thopter", "Thrull", "Tiefling", "Time Lord", "Tosk", "Toy", "Treefolk", "Tribble", "Trill",
    "Trilobite", "Triskelavite", "Troll", "Turtle", "Tyranid", "Unicorn", "Urzan", "Utrom",
    "Vampire", "Varmint", "Vau-N'Akat", "Vedalken", "Villain", "Volver", "Vorta", "Vulcan", "Wall",
    "Walrus", "Warlock", "Warrior", "Weasel", "Weird", "Werewolf", "Whale", "Wizard", "Wolf",
    "Wolverine", "Wombat", "Worm", "Wraith", "Wurm", "Xindi", "Yeti", "Zombie", "Zubera",
];

/// Is `name` a creature type (CR 205.3m)?
pub fn is_creature_type(name: &str) -> bool {
    CREATURE_TYPES.binary_search(&name).is_ok()
}

/// W1's instructions (`Ext::EoeW1`).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum EoeW1 {
    /// "Return each <filter> card exiled with this to the battlefield under
    /// its owner's control" (CR 607.2a: the cards the linked ability exiled,
    /// still linked after the source left, e.g. sacrificed to pay this
    /// ability's cost): every linked card in exile matching `filter` (a card
    /// in exile: its card's characteristics) enters under its owner's
    /// control; the others stay exiled and linked.
    LinkedToBattlefieldOwnersMatching { filter: Filter },
    /// "Exile all <filter> until this leaves the battlefield" (CR 610.3:
    /// each matching permanent, linked to the source, returns when it
    /// leaves; 610.3c: if it already left, nothing is exiled; a token
    /// exiled ceases to exist, 111.7).
    ExileEachUntilSourceLeaves { filter: Filter },
    /// "Put each card exiled with this into its owner's graveyard" (CR
    /// 607.2a): remembers how many were put into a graveyard this way
    /// (`Frame::x`, `Amount::Remembered`: "for each card put into a
    /// graveyard this way").
    LinkedToGraveyardsCounted,
    /// "Enchanted creature fights up to one target creature" (CR 701.14):
    /// the permanent the source (an Aura) is attached to and the target
    /// deal damage equal to their power to each other; no damage if either
    /// is gone or isn't a creature (701.14b), or if the Aura has left.
    HostFights { tgt: u8 },
}

impl EoeW1 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeW1::LinkedToBattlefieldOwnersMatching { .. }
            | EoeW1::ExileEachUntilSourceLeaves { .. }
            | EoeW1::LinkedToGraveyardsCounted
            | EoeW1::HostFights { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeW1::LinkedToBattlefieldOwnersMatching { .. }
            | EoeW1::ExileEachUntilSourceLeaves { .. }
            | EoeW1::LinkedToGraveyardsCounted
            | EoeW1::HostFights { .. } => None,
        }
    }
}

impl Game {
    pub(crate) fn exec_eoe_w1(&mut self, f: &mut Frame, op: &EoeW1) -> Flow {
        match op {
            EoeW1::LinkedToBattlefieldOwnersMatching { filter } => {
                let ctx = crate::cast::Ctx { controller: f.controller, source: Some(f.source) };
                let cards: Vec<_> = self
                    .linked_cards(f.source)
                    .into_iter()
                    .filter(|&c| self.objs[c.i()].zone == Zone::Exile && self.card_matches(filter, c, &ctx))
                    .collect();
                let refs: Vec<_> = cards.iter().map(|&c| self.obj_ref(c)).collect();
                self.links.retain(|l| !(l.source == f.source && refs.contains(&l.card)));
                for c in cards {
                    let owner = self.objs[c.i()].owner;
                    self.put_onto_battlefield(c, owner);
                }
                Flow::Next
            }
            EoeW1::ExileEachUntilSourceLeaves { filter } => {
                if self.objs_on(f, &On::Source).is_empty() {
                    return Flow::Next;
                }
                let matching = self.objs_on(f, &On::Each(filter.clone()));
                for c in matching {
                    if self.objs[c.i()].zone != Zone::Battlefield {
                        continue;
                    }
                    let card = self.move_card(c, Zone::Exile, Pos::Top);
                    if self.objs[card.idx.i()].zone == Zone::Exile {
                        self.links.push(crate::game::Link { source: f.source, card, until: true, back: Zone::Battlefield, until_monarch: None });
                    }
                }
                Flow::Next
            }
            EoeW1::LinkedToGraveyardsCounted => {
                let cards = self.linked_cards(f.source);
                self.links.retain(|l| l.source != f.source);
                let mut n = 0;
                for c in cards {
                    let r = self.move_card(c, Zone::Graveyard, Pos::Top);
                    if self.objs[r.idx.i()].zone == Zone::Graveyard {
                        n += 1;
                    }
                }
                f.x = n;
                Flow::Next
            }
            EoeW1::HostFights { tgt } => {
                let Some(host) = self.objs_on(f, &On::Source).first().and_then(|&a| self.objs[a.i()].attached) else { return Flow::Next };
                let other = self.objs_on(f, &On::Target(*tgt));
                if let Some(&b) = other.first() {
                    let a = host.idx;
                    if self.live(host) && self.objs[a.i()].zone == Zone::Battlefield && self.is_creature(a) && self.is_creature(b) {
                        let (sa, sb) = (self.damage_source_of(a), self.damage_source_of(b));
                        let (pa, pb) = (self.power(a), self.power(b));
                        let rb = self.obj_ref(b);
                        // Both deal damage at once (CR 701.14a).
                        self.deal_damage(sa, Target::Obj(rb), pa);
                        if a != b {
                            self.deal_damage(sb, Target::Obj(host), pb);
                        }
                    }
                }
                Flow::Next
            }
        }
    }
}
