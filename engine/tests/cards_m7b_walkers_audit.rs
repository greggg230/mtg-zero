//! M7b: the supported cards that name planeswalkers, "any target" or
//! "permanent", against planeswalkers (CR 115.4, 120.3c, 306; ENGINE.md
//! 24): before M7b no planeswalker existed, so these paths never ran.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const JACE: &str = "Jace, the Mind Sculptor";
const W6: &str = "Wrenn and Six";

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn loyalty(g: &Game, c: CardIdx) -> u16 {
    g.counters_on(c, Counter::Loyalty)
}

fn settle(g: &mut Game) {
    while let Some(d) = g.pending_decision() {
        if d.kind == DecisionKind::Priority || d.options.len() != 1 {
            return;
        }
        let c = d.options[0].choice;
        g.choose(c);
    }
}

/// Answer with `answer` (else first options) until a player has priority.
fn answer_until_priority(g: &mut Game, answer: &dyn Fn(&Game) -> Option<Choice>) {
    while !at_priority(g) {
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress");
}

fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

/// Cast `name` from P0's hand and resolve it, targeting `t` (and choosing
/// modes with `answer`).
fn cast_at(g: &mut Game, name: &str, t: Choice, answer: impl Fn(&Game) -> Option<Choice>) {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    settle(g);
    let pick = |g: &Game| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        _ => answer(g),
    };
    answer_until_priority(g, &pick);
    resolve_with(g, pick);
}

fn no(_: &Game) -> Option<Choice> {
    None
}

/// Attack `jace` with P0's `attacker` and finish the combat.
fn attack(g: &mut Game, attacker: &str, walker: CardIdx) {
    let a = bf(g, 0, attacker);
    g.choose(Choice::Card(a));
    g.choose(g.tgt(walker));
    quiet_until(g, |g| g.turn().step == Step::Main2 && at_priority(g));
}

#[test]
fn removal_of_a_creature_or_planeswalker_and_of_a_permanent_hits_planeswalkers() {
    for (spell, lands) in [
        ("Bitter Triumph", &["Swamp", "Swamp"][..]),
        ("Vindicate", &["Swamp", "Plains", "Plains"][..]),
        ("Bone Shards", &["Swamp"][..]),
        ("Baleful Mastery", &["Swamp", "Swamp", "Swamp", "Swamp"][..]),
    ] {
        let mut g = scen()
            .battlefield(0, lands)
            .hand(0, &[spell, "Island", "Test Bear"])
            .battlefield(1, &[JACE])
            .at(Step::Main1, 0)
            .start();
        let jace = bf(&g, 1, JACE);
        let t = g.tgt(jace);
        cast_at(&mut g, spell, t, |g| match g.kind() {
            // Bitter Triumph / Bone Shards: discard (or pay) a card; Baleful
            // Mastery: the mana cost.
            Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
            _ => None,
        });
        assert_ne!(g.obj(jace).zone, Zone::Battlefield, "{spell}");
    }
}

#[test]
fn abrupt_decay_respects_a_planeswalkers_mana_value() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Forest"])
        .hand(0, &["Abrupt Decay"])
        .battlefield(1, &[JACE, W6])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    let w6 = bf(&g, 1, W6);
    let c = g.card(0, "Abrupt Decay", Zone::Hand);
    g.choose(Choice::Cast(c));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(jace)), "mana value 4");
    g.choose(g.tgt(w6));
    answer_until_priority(&mut g, &no);
    resolve_with(&mut g, no);
    assert_eq!(g.obj(w6).zone, Zone::Graveyard);
}

#[test]
fn burn_to_any_target_damages_planeswalkers() {
    // Kolaghan's Command's 2 damage mode; Lightning Bolt in rules_walkers.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain"])
        .hand(0, &["Kolaghan's Command"])
        .battlefield(1, &[JACE])
        .hand(1, &["Island"])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    let modes = std::cell::Cell::new(0);
    let t = g.tgt(jace);
    cast_at(&mut g, "Kolaghan's Command", t, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            // Modes 1 (discard) and 3 (2 damage).
            modes.set(modes.get() + 1);
            Some(if g.has_option(Choice::Number(1)) && modes.get() == 1 { Choice::Number(1) } else { Choice::Number(3) })
        }
        Some(DecisionKind::Target) => Some(Choice::Target(Target::Player(1))),
        _ => None,
    });
    assert_eq!(loyalty(&g, jace), 1);
}

#[test]
fn pingers_damage_planeswalkers() {
    // Walking Ballista, Goblin Bombardment, Orcish Bowmasters.
    let mut g = scen()
        .battlefield(0, &["Walking Ballista", "Goblin Bombardment", "Test Bear"])
        .counters(0, "Walking Ballista", Counter::P1P1, 1)
        .battlefield(1, &[JACE])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    let wb = bf(&g, 0, "Walking Ballista");
    let gb = bf(&g, 0, "Goblin Bombardment");
    let bear = bf(&g, 0, "Test Bear");
    // Ballista's "remove a +1/+1 counter" is its second ability.
    for (c, i) in [(wb, 1u8), (gb, 0)] {
        g.choose(Choice::Ability(c, AbilitySlot::Activated(i)));
        answer_until_priority(&mut g, &|g| match g.kind() {
            Some(DecisionKind::Target) => Some(g.tgt(jace)),
            Some(DecisionKind::Sacrifice) => Some(Choice::Card(bear)),
            _ => None,
        });
        resolve_with(&mut g, no);
    }
    assert_eq!(loyalty(&g, jace), 1);
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Orcish Bowmasters"])
        .battlefield(1, &[JACE])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    let c = g.card(0, "Orcish Bowmasters", Zone::Hand);
    g.choose(Choice::Cast(c));
    answer_until_priority(&mut g, &no);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(jace)));
    assert_eq!(loyalty(&g, jace), 2);
}

#[test]
fn fury_divides_damage_among_creatures_and_planeswalkers() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 5])
        .hand(0, &["Fury"])
        .battlefield(1, &[JACE, "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    let bear = bf(&g, 1, "Test Bear");
    let c = g.card(0, "Fury", Zone::Hand);
    g.choose(Choice::Cast(c));
    answer_until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        _ => None,
    });
    let mut first = true;
    for _ in 0..100 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        let c = match g.kind() {
            Some(DecisionKind::Target) if g.has_option(g.tgt(jace)) => g.tgt(jace),
            Some(DecisionKind::Target) if g.has_option(g.tgt(bear)) => g.tgt(bear),
            Some(DecisionKind::Target) => Choice::Pass,
            Some(DecisionKind::DivideDamage) if first => {
                first = false;
                Choice::Number(3)
            }
            _ => g.pending_decision().unwrap().options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.obj(jace).zone, Zone::Graveyard, "3 to Jace");
    assert_eq!(g.obj(bear).damage, 1);
}

#[test]
fn fiery_confluence_each_creature_spares_planeswalkers() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 4])
        .hand(0, &["Fiery Confluence"])
        .battlefield(1, &[JACE])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    cast_at(&mut g, "Fiery Confluence", Choice::Pass, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(if g.has_option(Choice::Number(0)) { Choice::Number(0) } else { Choice::Pass }),
        _ => None,
    });
    assert_eq!(loyalty(&g, jace), 3);
}

#[test]
fn councils_judgment_and_pyroblast_take_planeswalkers() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 3])
        .hand(0, &["Council's Judgment"])
        .battlefield(1, &[JACE])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    cast_at(&mut g, "Council's Judgment", Choice::Pass, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(g.find_card(1, JACE, Zone::Battlefield)?)),
        _ => None,
    });
    assert_eq!(g.obj(jace).zone, Zone::Exile);
    // Pyroblast: "destroy target permanent if it's blue".
    let mut g = scen().battlefield(0, &["Mountain"]).hand(0, &["Pyroblast"]).battlefield(1, &[JACE]).at(Step::Main1, 0).start();
    let jace = bf(&g, 1, JACE);
    let t = g.tgt(jace);
    cast_at(&mut g, "Pyroblast", t, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        _ => None,
    });
    assert_eq!(g.obj(jace).zone, Zone::Graveyard);
}

#[test]
fn edicts_can_take_planeswalkers() {
    // Sheoldred's Edict's planeswalker mode; Archon of Cruelty's "a creature
    // or planeswalker".
    let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Sheoldred's Edict"]).battlefield(1, &[JACE]).at(Step::Main1, 0).start();
    let jace = bf(&g, 1, JACE);
    cast_at(&mut g, "Sheoldred's Edict", Choice::Pass, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(2)),
        _ => None,
    });
    assert_eq!(g.obj(jace).zone, Zone::Graveyard);
    let mut g = scen()
        .battlefield(0, &["Swamp"; 8])
        .hand(0, &["Archon of Cruelty"])
        .battlefield(1, &[JACE, "Test Bear"])
        .hand(1, &["Island"])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    cast_at(&mut g, "Archon of Cruelty", Choice::Target(Target::Player(1)), |g| match g.kind() {
        Some(DecisionKind::Sacrifice) => {
            assert!(g.has_option(Choice::Card(g.find_card(1, JACE, Zone::Battlefield).unwrap())));
            Some(Choice::Card(g.find_card(1, JACE, Zone::Battlefield).unwrap()))
        }
        _ => None,
    });
    assert_eq!(g.obj(jace).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1);
}

#[test]
fn psychic_frog_draws_on_combat_damage_to_a_planeswalker_and_questing_beast_redirects() {
    let mut g = scen().battlefield(0, &["Psychic Frog"]).battlefield(1, &[JACE]).at(Step::DeclareAttackers, 0).start();
    let jace = bf(&g, 1, JACE);
    let hand = g.player(0).hand.len();
    attack(&mut g, "Psychic Frog", jace);
    assert_eq!(loyalty(&g, jace), 2);
    assert_eq!(g.player(0).hand.len(), hand + 1, "a player or planeswalker");
    // Questing Beast: combat damage to the opponent, that much to target
    // planeswalker that player controls.
    let mut g = scen().battlefield(0, &["Questing Beast"]).battlefield(1, &["Elspeth, Storm Slayer"]).at(Step::DeclareAttackers, 0).start();
    let qb = bf(&g, 0, "Questing Beast");
    let el = bf(&g, 1, "Elspeth, Storm Slayer");
    g.choose(Choice::Card(qb));
    g.choose(Choice::Target(Target::Player(1)));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(1), 16);
    assert_eq!(g.obj(el).zone, Zone::Battlefield);
    assert_eq!(loyalty(&g, el), 1, "4 damage to Elspeth's 5");
}

#[test]
fn clarion_conqueror_stops_loyalty_abilities_and_mox_amber_sees_planeswalkers() {
    let g = scen().battlefield(0, &[JACE]).battlefield(1, &["Clarion Conqueror"]).at(Step::Main1, 0).start();
    let jace = bf(&g, 0, JACE);
    assert!(!g.has_option(Choice::Ability(jace, AbilitySlot::Activated(1))), "CR 602.5");
    // Mox Amber: a colour among legendary creatures and planeswalkers.
    let g = scen().battlefield(0, &["Mox Amber", "Wrenn and Six"]).hand(0, &["Test Bear"]).at(Step::Main1, 0).start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bear)), "{{1}}{{G}} needs two mana");
    let g = scen().battlefield(0, &["Mox Amber", W6, "Forest"]).hand(0, &["Test Bear"]).at(Step::Main1, 0).start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bear)), "Mox Amber makes G or R");
}

#[test]
fn subtlety_puts_a_planeswalker_spell_on_top() {
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Subtlety"])
        .battlefield(1, &["Island"; 4])
        .hand(1, &[JACE])
        .at(Step::Main1, 1)
        .start();
    let j = g.card(1, JACE, Zone::Hand);
    g.choose(Choice::Cast(j));
    answer_until_priority(&mut g, &no);
    g.pass(); // P1
    // P0 flashes Subtlety in (its mana cost).
    let s = g.card(0, "Subtlety", Zone::Hand);
    g.choose(Choice::Cast(s));
    answer_until_priority(&mut g, &|g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        _ => None,
    });
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).map(|o| o.choice),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(j).zone, Zone::Library);
    assert_eq!(*g.player(1).library.last().unwrap(), j);
}
