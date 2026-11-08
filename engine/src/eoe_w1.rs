//! Edge of Eternities, W1 (ENGINE.md 99.11): the ops the cards whose card IR
//! mtg-bot's parser fixed need. Named after the rules actions, each citing
//! its rule; never after a card.

use crate::db::{types, Counter, Filter};
use crate::decision::{act, Choice, DecisionKind, Opt};
use crate::flow::Task;
use crate::game::{Game, Pos};
use crate::ids::{Target, Zone};
use crate::program::{On, Who};
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

impl Game {
    /// "An artifact entered the battlefield under your control this turn"
    /// (`Player::artifact_entered`): as its `Enters` event happens, with
    /// what it entered as (a copy's characteristics too, CR 707.5).
    pub(crate) fn note_entered(&mut self, c: crate::ids::CardIdx) {
        if self.objs[c.i()].zone == Zone::Battlefield && self.is_type(c, types::ARTIFACT) {
            let p = self.objs[c.i()].controller;
            self.players[p as usize].artifact_entered = true;
        }
    }
}

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
    /// "<Who> chooses a <filter> they control and exiles it" (CR 701.13:
    /// chosen as it resolves, CR 608.2d; Sothera, the Supervoid): with
    /// `linked`, the card is exiled with the source (CR 607.2a, no
    /// duration: it stays linked after the source leaves). Choice: one of
    /// their matching permanents (none: nothing happens).
    ChooseOwnExileLinked { who: Who, filter: Filter, linked: bool },
    /// "Put a creature card exiled with this onto the battlefield under
    /// your control with n additional <kind> counters on it" (CR 607.2a;
    /// the counters as it enters, 614.1c): the source may have left (the
    /// link remains). Choice: one of the linked creature cards (one: no
    /// decision); with more than the 32 exile slots, pages of 31 and `PASS`
    /// for the next page (`Frame::i`), so every card stays reachable.
    LinkedCreatureToBattlefieldWith { counter: Counter, n: u8 },
}

impl EoeW1 {
    /// Does the op read its triggered ability's event?
    pub fn uses_event(&self) -> bool {
        match self {
            EoeW1::LinkedToBattlefieldOwnersMatching { .. }
            | EoeW1::ExileEachUntilSourceLeaves { .. }
            | EoeW1::LinkedToGraveyardsCounted
            | EoeW1::HostFights { .. }
            | EoeW1::ChooseOwnExileLinked { .. }
            | EoeW1::LinkedCreatureToBattlefieldWith { .. } => false,
        }
    }

    /// The forward jump target of a control-flow op.
    pub fn jump_target(&mut self) -> Option<&mut u16> {
        match self {
            EoeW1::LinkedToBattlefieldOwnersMatching { .. }
            | EoeW1::ExileEachUntilSourceLeaves { .. }
            | EoeW1::LinkedToGraveyardsCounted
            | EoeW1::HostFights { .. }
            | EoeW1::ChooseOwnExileLinked { .. }
            | EoeW1::LinkedCreatureToBattlefieldWith { .. } => None,
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
                // "Enchanted creature": the permanent the Aura is attached to,
                // or, if the Aura has left, was attached to as it last
                // existed on the battlefield (CR 608.2h).
                let host = match self.objs_on(f, &On::Source).first() {
                    Some(&a) => self.objs[a.i()].attached,
                    None => self.lki_attached.iter().find(|(o, _)| *o == f.source).map(|&(_, h)| h),
                };
                let Some(host) = host else { return Flow::Next };
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
            EoeW1::ChooseOwnExileLinked { who, filter, linked } => {
                let Some(p) = self.who(f, *who) else { return Flow::Next };
                match self.answer.take() {
                    Some(Choice::Card(c)) => {
                        if self.objs[c.i()].zone == Zone::Battlefield {
                            self.note(|g| format!("P{p} exiles {}", g.name(c)));
                            let card = self.move_card(c, Zone::Exile, Pos::Top);
                            if *linked && self.objs[card.idx.i()].zone == Zone::Exile {
                                self.links.push(crate::game::Link { source: f.source, card, until: false, back: Zone::Battlefield, until_monarch: None });
                            }
                        }
                        Flow::Next
                    }
                    Some(other) => unreachable!("exile choice {other:?}"),
                    None => {
                        let ctx = crate::cast::Ctx { controller: p, source: Some(f.source) };
                        let opts: Vec<Opt> = self
                            .permanents_of(p)
                            .filter(|&c| self.card_matches(filter, c, &ctx))
                            .map(|c| Opt { action: self.perm_action(p, c), choice: Choice::Card(c) })
                            .collect();
                        if opts.is_empty() {
                            return Flow::Next;
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChoosePermanent, opts);
                        Flow::Suspend
                    }
                }
            }
            EoeW1::LinkedCreatureToBattlefieldWith { counter, n } => {
                f.made = None;
                let p = f.controller;
                let cands: Vec<_> = self.linked_cards(f.source).into_iter().filter(|&c| self.def_of(c).is(types::CREATURE)).collect();
                let answer = self.answer.take();
                let pick = match answer {
                    Some(Choice::Card(c)) => c,
                    None if cands.len() == 1 => cands[0],
                    Some(Choice::Pass) | None => {
                        if cands.is_empty() {
                            return Flow::Next;
                        }
                        if answer == Some(Choice::Pass) {
                            f.i += 1;
                        }
                        let all = act::EXILE_N as usize;
                        let page = all - 1;
                        let (start, end, more) = if cands.len() <= all {
                            (0, cands.len(), false)
                        } else {
                            let start = (f.i as usize * page).min(cands.len() - 1);
                            let end = (start + page).min(cands.len());
                            (start, end, end < cands.len())
                        };
                        let mut opts: Vec<Opt> = cands[start..end]
                            .iter()
                            .enumerate()
                            .map(|(k, &c)| Opt { action: act::EXILE + k as u16, choice: Choice::Card(c) })
                            .collect();
                        if more {
                            opts.push(Opt { action: act::PASS, choice: Choice::Pass });
                        }
                        self.ask(Task::Run(f.clone()), p, DecisionKind::ChooseExiled, opts);
                        return Flow::Suspend;
                    }
                    Some(other) => unreachable!("linked card answer {other:?}"),
                };
                let r = self.obj_ref(pick);
                self.links.retain(|l| !(l.source == f.source && l.card == r));
                // (With the counters as it enters, CR 614.1c.)
                self.entering_counters = vec![(*counter, *n as crate::num::Count)];
                self.put_onto_battlefield(pick, p);
                self.entering_counters.clear();
                if self.objs[pick.i()].zone == Zone::Battlefield {
                    f.made = Some(pick);
                }
                Flow::Next
            }
        }
    }
}
