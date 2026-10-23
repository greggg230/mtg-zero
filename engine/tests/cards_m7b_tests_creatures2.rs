//! M7b card tests, creatures (K-W): Knight of Autumn, Mole Man, Oliphaunt,
//! Ouroboroid, Phelia, Phlage, Ravenous Robots, Spectacular Spider-Man,
//! Spellseeker, Stoneforge Mystic, Sylvan Safekeeper, Titania, Trinket
//! Mage, Witherbloom Apprentice; each against its Oracle text.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap()
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

fn subtype(g: &Game, c: CardIdx, s: &str) -> bool {
    db().subtype_id(s).is_some_and(|id| g.has_subtype(c, id))
}

fn pay(g: &mut Game) {
    g.drive_until(|g| {
        at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
    });
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| {
            at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
        });
    }
}

fn cast_card(g: &mut Game, c: CardIdx, alt: u8) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::ChooseAltCost) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(alt));
    }
    pay(g);
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    cast_card(g, c, 0);
}

fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
            return;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
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

/// Declare `attackers` (P0's declare attackers decision pending), no
/// blocks; answer the rest with `answer` until `stop`.
fn attack_until(
    g: &mut Game,
    attackers: &[CardIdx],
    answer: impl Fn(&Game) -> Option<Choice>,
    stop: impl Fn(&Game) -> bool,
) {
    assert_eq!(g.kind(), Some(DecisionKind::DeclareAttackers));
    let mut sorted = attackers.to_vec();
    sorted.sort_by_key(|&c| g.battlefield().iter().position(|&x| x == c));
    for &a in &sorted {
        g.choose(Choice::Card(a));
    }
    g.choose(Choice::Pass);
    for _ in 0..1000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareBlockers | DecisionKind::DeclareAttackers => Choice::Pass,
            _ => answer(g).unwrap_or(d.options[0].choice),
        };
        g.choose(c);
    }
    panic!("attack_until: no progress");
}

fn main2(g: &Game) -> bool {
    g.turn().step == Step::Main2 && at_priority(g) && g.stack().is_empty()
}

fn offered_defs(g: &Game) -> Vec<u16> {
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::SearchLibrary, "{:?}", g.options_desc());
    let mut v: Vec<u16> =
        d.options.iter().filter_map(|o| if let Choice::Def(x) = o.choice { Some(x) } else { None }).collect();
    v.sort();
    v
}

fn defs(names: &[&str]) -> Vec<u16> {
    let mut v: Vec<u16> = names.iter().map(|n| def(n)).collect();
    v.sort();
    v
}

// ---- Knight of Autumn -----------------------------------------------------------

#[test]
fn knight_of_autumn_enters_with_one_of_three_modes() {
    // "When this creature enters, choose one - Put two +1/+1 counters on
    // this creature; Destroy target artifact or enchantment; You gain 4
    // life."
    for mode in 0..3u8 {
        let mut g = scen()
            .battlefield(0, &["Forest", "Plains", "Plains"])
            .hand(0, &["Knight of Autumn"])
            .battlefield(1, &["Sol Ring", "Test Bear"])
            .at(Step::Main1, 0)
            .start();
        cast(&mut g, 0, "Knight of Autumn");
        let ring = bf(&g, 1, "Sol Ring");
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => {
                assert_eq!(g.pending_decision().unwrap().options.len(), 3);
                Some(Choice::Number(mode))
            }
            Some(DecisionKind::Target) => {
                assert!(!g.has_option(g.tgt(bf(g, 1, "Test Bear"))), "an artifact or enchantment");
                Some(g.tgt(ring))
            }
            _ => None,
        });
        let k = bf(&g, 0, "Knight of Autumn");
        match mode {
            0 => assert_eq!((g.pt(k), g.counters_on(k, Counter::P1P1)), ((4, 3), 2)),
            1 => assert_eq!(g.obj(ring).zone, Zone::Graveyard),
            _ => assert_eq!(g.life(0), 24),
        }
        if mode != 1 {
            assert_eq!(g.obj(ring).zone, Zone::Battlefield);
        }
        if mode != 0 {
            assert_eq!(g.pt(k), (2, 1));
        }
    }
    // 700.2: with no artifact or enchantment, the destroy mode can't be
    // chosen.
    let mut g = scen()
        .battlefield(0, &["Forest", "Plains", "Plains"])
        .hand(0, &["Knight of Autumn"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Knight of Autumn");
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode));
    assert!(g.has_option(Choice::Number(0)) && !g.has_option(Choice::Number(1)) && g.has_option(Choice::Number(2)));
}

// ---- Mole Man -----------------------------------------------------------------------

#[test]
fn mole_man_plays_lands_from_the_graveyard_and_makes_moloids_on_landfall() {
    // "You may play lands from your graveyard. Landfall - Whenever a land
    // you control enters, create a 1/1 green Minion creature token named
    // Moloid with 'Whenever this token attacks, you may mill a card.'"
    let mut g = scen()
        .battlefield(0, &["Mole Man, Moloid Master"])
        .graveyard(0, &["Forest"])
        .hand(0, &["Island"])
        .at(Step::Main1, 0)
        .start();
    let mm = bf(&g, 0, "Mole Man, Moloid Master");
    assert_eq!(g.pt(mm), (1, 1));
    let forest = g.card(0, "Forest", Zone::Graveyard);
    let island = g.card(0, "Island", Zone::Hand);
    assert!(g.has_option(Choice::Play(forest)), "a land from the graveyard");
    g.choose(Choice::Play(forest));
    settle(&mut g);
    assert_eq!(g.obj(forest).zone, Zone::Battlefield);
    assert!(!g.has_option(Choice::Play(island)), "it used the land drop (305.2)");
    let moloid = bf(&g, 0, "Moloid");
    assert_eq!(g.pt(moloid), (1, 1));
    assert_eq!(g.colors_of(moloid), 1 << engine::mana::G, "green");
    assert!(subtype(&g, moloid, "Minion"));
    // The Moloid attacks next turn: "you may mill a card".
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let lib = g.player(0).library.len();
    let top = *g.player(0).library.last().unwrap();
    attack_until(
        &mut g,
        &[moloid],
        |g| {
            if g.kind() == Some(DecisionKind::YesNo) {
                Some(Choice::Yes)
            } else {
                None
            }
        },
        main2,
    );
    assert_eq!(g.obj(top).zone, Zone::Graveyard, "milled the top card");
    assert_eq!(g.player(0).library.len(), lib - 1);
    // An opponent's land doesn't trigger it.
    let mut g = scen().battlefield(0, &["Mole Man, Moloid Master"]).hand(1, &["Forest"]).at(Step::Main1, 1).start();
    let f = g.card(1, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Moloid", Zone::Battlefield) + g.count_in(1, "Moloid", Zone::Battlefield), 0);
    // Without Mole Man, no graveyard land plays.
    let g = scen().graveyard(0, &["Forest"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(Choice::Play(g.card(0, "Forest", Zone::Graveyard))));
}

// ---- Oliphaunt ---------------------------------------------------------------------

#[test]
fn oliphaunt_pumps_another_attacker_you_control_and_mountaincycles() {
    // "Trample. Whenever this creature attacks, another target creature you
    // control gets +2/+0 and gains trample until end of turn."
    let mut g = scen()
        .battlefield(0, &["Oliphaunt", "Test Bear"])
        .battlefield(1, &["Test Reacher"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let oli = bf(&g, 0, "Oliphaunt");
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(oli), (6, 4));
    assert!(g.has_kw(oli, Kw::Trample));
    attack_until(
        &mut g,
        &[oli],
        |g| {
            if g.kind() == Some(DecisionKind::Target) {
                assert!(!g.has_option(g.tgt(bf(g, 0, "Oliphaunt"))), "another");
                assert!(!g.has_option(g.tgt(bf(g, 1, "Test Reacher"))), "you control");
                Some(g.tgt(bf(g, 0, "Test Bear")))
            } else {
                None
            }
        },
        main2,
    );
    assert_eq!(g.pt(bear), (4, 2));
    assert!(g.has_kw(bear, Kw::Trample));
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 1 && at_priority(g));
    assert_eq!(g.pt(bear), (2, 2), "until end of turn");
    assert!(!g.has_kw(bear, Kw::Trample));
    // "Mountaincycling {1}": a Mountain card (Plateau is one), revealed.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island"])
        .hand(0, &["Oliphaunt"])
        .library(0, &["Plains", "Plateau", "Mountain", "Lightning Bolt"])
        .library(1, &["Forest"; 5])
        .at(Step::Upkeep, 0)
        .start();
    let o = g.card(0, "Oliphaunt", Zone::Hand);
    g.choose(Choice::Ability(o, AbilitySlot::Hand(0)));
    pay(&mut g);
    assert_eq!(g.obj(o).zone, Zone::Graveyard);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(offered_defs(&g), defs(&["Plateau", "Mountain"]));
    g.choose(Choice::Def(def("Plateau")));
    let p = g.card(0, "Plateau", Zone::Hand);
    assert!(g.knows(1, p), "revealed");
}

// ---- Ouroboroid --------------------------------------------------------------------

#[test]
fn ouroboroid_puts_counters_equal_to_its_power_on_each_creature_you_control() {
    // "At the beginning of combat on your turn, put X +1/+1 counters on
    // each creature you control, where X is this creature's power."
    let mut g = scen()
        .battlefield(0, &["Ouroboroid", "Test Bear"])
        .battlefield(1, &["Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let o = bf(&g, 0, "Ouroboroid");
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(o), (1, 3));
    quiet_until(&mut g, |g| g.turn().step == Step::BeginCombat && !g.stack().is_empty());
    settle(&mut g);
    assert_eq!(g.counters_on(o, Counter::P1P1), 1);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
    assert_eq!(g.counters_on(bf(&g, 1, "Test Reacher"), Counter::P1P1), 0, "you control");
    assert_eq!(g.pt(o), (2, 4));
    // Next turn X = 2; not on the opponent's turn.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.turn().step == Step::BeginCombat && !g.stack().is_empty());
    assert_eq!(g.counters_on(o, Counter::P1P1), 1, "P1's turn added nothing");
    settle(&mut g);
    assert_eq!(g.counters_on(o, Counter::P1P1), 3);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 3);
}

// ---- Phelia ------------------------------------------------------------------------

#[test]
fn phelia_flickers_a_nonland_permanent_until_the_end_step() {
    // "Flash. Whenever Phelia attacks, exile up to one other target nonland
    // permanent. At the beginning of the next end step, return that card to
    // the battlefield under its owner's control. If it entered under your
    // control, put a +1/+1 counter on Phelia."
    let mut g = scen()
        .battlefield(0, &["Phelia, Exuberant Shepherd", "Sol Ring"])
        .battlefield(1, &["Test Bear", "Forest"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let phelia = bf(&g, 0, "Phelia, Exuberant Shepherd");
    let bear = bf(&g, 1, "Test Bear");
    assert!(g.has_kw(phelia, Kw::Flash));
    attack_until(
        &mut g,
        &[phelia],
        |g| {
            if g.kind() == Some(DecisionKind::Target) {
                assert!(!g.has_option(g.tgt(bf(g, 0, "Phelia, Exuberant Shepherd"))), "other");
                assert!(!g.has_option(g.tgt(bf(g, 1, "Forest"))), "nonland");
                assert!(g.has_option(Choice::Pass), "up to one");
                Some(g.tgt(bf(g, 1, "Test Bear")))
            } else {
                None
            }
        },
        main2,
    );
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.life(1), 18, "unblocked");
    quiet_until(&mut g, |g| g.turn().step == Step::Cleanup || g.turn().active == 1);
    let bear = bf(&g, 1, "Test Bear");
    assert_eq!(g.obj(bear).controller, 1, "under its owner's control");
    assert_eq!(g.counters_on(phelia, Counter::P1P1), 0, "it entered under P1's control");
    // Your own permanent: it returns under your control, Phelia grows.
    let mut g =
        scen().battlefield(0, &["Phelia, Exuberant Shepherd", "Sol Ring"]).at(Step::DeclareAttackers, 0).start();
    let phelia = bf(&g, 0, "Phelia, Exuberant Shepherd");
    attack_until(
        &mut g,
        &[phelia],
        |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bf(g, 0, "Sol Ring"))),
        main2,
    );
    assert_eq!(g.count_in(0, "Sol Ring", Zone::Exile), 1);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.count_in(0, "Sol Ring", Zone::Battlefield), 1);
    assert_eq!(g.counters_on(phelia, Counter::P1P1), 1);
}

// ---- Phlage --------------------------------------------------------------------------

#[test]
fn phlage_hard_cast_deals_three_gains_three_and_is_sacrificed() {
    // "When Phlage enters, sacrifice it unless it escaped. Whenever Phlage
    // enters or attacks, it deals 3 damage to any target and you gain 3
    // life."
    let mut g = scen()
        .battlefield(0, &["Mountain", "Plains", "Plains"])
        .hand(0, &["Phlage, Titan of Fire's Fury"])
        .at(Step::Main1, 0)
        .start();
    let ph = g.card(0, "Phlage, Titan of Fire's Fury", Zone::Hand);
    cast_card(&mut g, ph, 0);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(player(1))).then(|| player(1)));
    assert_eq!(g.obj(ph).zone, Zone::Graveyard, "not escaped: sacrificed");
    assert_eq!(g.life(1), 17);
    assert_eq!(g.life(0), 23);
}

#[test]
fn phlage_escapes_for_rrww_and_five_other_cards_then_attacks() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Plains", "Plains"])
        .graveyard(0, &["Phlage, Titan of Fire's Fury", "Island", "Island", "Lightning Bolt", "Test Bear", "Sol Ring"])
        .battlefield(1, &["Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let ph = g.card(0, "Phlage, Titan of Fire's Fury", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(ph)), "Escape - {{R}}{{R}}{{W}}{{W}}, exile five other cards");
    g.choose(Choice::Cast(ph));
    let mut exiled = 0;
    for _ in 0..40 {
        g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
        match g.kind() {
            Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseAltCost) => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
            Some(DecisionKind::ChooseGraveyardCard) => {
                let c = g.pending_decision().unwrap().options[0].choice;
                assert_ne!(c, Choice::Card(ph), "other cards");
                g.choose(c);
                exiled += 1;
            }
            _ => break,
        }
    }
    assert_eq!(exiled, 5);
    let reacher = bf(&g, 1, "Test Reacher");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(reacher)));
    let ph = bf(&g, 0, "Phlage, Titan of Fire's Fury");
    assert_eq!(g.pt(ph), (6, 6), "escaped: it stays");
    assert_eq!(g.obj(reacher).zone, Zone::Graveyard, "3 damage to a 1/3");
    assert_eq!(g.life(0), 23);
    // Attacks next turn: 3 damage and 3 life again.
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t0 + 2 && g.kind() == Some(DecisionKind::DeclareAttackers));
    attack_until(&mut g, &[ph], |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)), main2);
    assert_eq!(g.life(1), 20 - 3 - 6);
    assert_eq!(g.life(0), 26);
}

// ---- Ravenous Robots ---------------------------------------------------------------

#[test]
fn ravenous_robots_makes_a_robot_per_artifact_spell_and_hastes_tokens() {
    // "Whenever you cast an artifact spell, create a 1/1 colorless Robot
    // artifact creature token. {R}, {T}: Creature tokens you control gain
    // haste until end of turn."
    let mut g = scen()
        .battlefield(0, &["Ravenous Robots", "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Sol Ring", "Test Bear", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let rr = bf(&g, 0, "Ravenous Robots");
    assert_eq!(g.pt(rr), (2, 1));
    cast(&mut g, 0, "Sol Ring");
    settle(&mut g);
    let robots: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.name(c) == "Robot").collect();
    assert_eq!(robots.len(), 1, "one artifact spell");
    let robot = robots[0];
    assert_eq!(g.pt(robot), (1, 1));
    assert!(g.is_type(robot, engine::db::types::ARTIFACT) && g.colors_of(robot) == 0);
    cast(&mut g, 0, "Lightning Bolt");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)));
    assert_eq!(g.count_in(0, "Robot", Zone::Battlefield), 1, "an instant is no artifact spell");
    // Haste for tokens.
    g.choose(act(rr, 0));
    pay(&mut g);
    settle(&mut g);
    assert!(g.has_kw(robot, Kw::Haste));
    assert!(!g.has_kw(rr, Kw::Haste), "tokens only");
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.has_option(Choice::Card(robot)), "the new token can attack");
}

// ---- Spectacular Spider-Man -----------------------------------------------------------

#[test]
fn spectacular_spider_man_flies_for_one_and_sacrifices_to_protect_the_team() {
    // "Flash. {1}: Spectacular Spider-Man gains flying until end of turn.
    // {1}, Sacrifice Spectacular Spider-Man: Creatures you control gain
    // hexproof and indestructible until end of turn."
    let mut g = scen()
        .battlefield(0, &["Spectacular Spider-Man", "Test Bear", "Island", "Island"])
        .battlefield(1, &["Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let ssm = bf(&g, 0, "Spectacular Spider-Man");
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(ssm), (3, 2));
    assert!(g.has_kw(ssm, Kw::Flash) && g.is_legendary(ssm));
    g.choose(act(ssm, 0));
    pay(&mut g);
    settle(&mut g);
    assert!(g.has_kw(ssm, Kw::Flying));
    g.choose(act(ssm, 1));
    pay(&mut g);
    assert_eq!(g.obj(ssm).zone, Zone::Graveyard);
    settle(&mut g);
    assert!(g.has_kw(bear, Kw::Hexproof) && g.has_kw(bear, Kw::Indestructible));
    assert!(!g.has_kw(bf(&g, 1, "Test Reacher"), Kw::Hexproof), "creatures you control");
}

// ---- Spellseeker, Trinket Mage, Stoneforge Mystic ------------------------------------------

#[test]
fn spellseeker_finds_an_instant_or_sorcery_with_mana_value_two_or_less() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Spellseeker"])
        .library(0, &["Lightning Bolt", "Counterspell", "Mystic Confluence", "Test Bear", "Sol Ring", "Ponder"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Spellseeker");
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "you may");
    g.choose(Choice::Yes);
    assert_eq!(offered_defs(&g), defs(&["Lightning Bolt", "Counterspell", "Ponder"]));
    g.choose(Choice::Def(def("Counterspell")));
    settle(&mut g);
    let cs = g.card(0, "Counterspell", Zone::Hand);
    assert!(g.knows(1, cs), "revealed");
    assert_eq!(g.pt(bf(&g, 0, "Spellseeker")), (1, 1));
}

#[test]
fn trinket_mage_finds_an_artifact_with_mana_value_one_or_less() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Trinket Mage"])
        .library(0, &["Sol Ring", "Mox Pearl", "Mind Stone", "Lightning Bolt", "Test Zero"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Trinket Mage");
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    assert_eq!(offered_defs(&g), defs(&["Sol Ring", "Mox Pearl"]), "Test Zero is no artifact; Mind Stone is 2");
    g.choose(Choice::Def(def("Sol Ring")));
    settle(&mut g);
    assert!(g.knows(1, g.card(0, "Sol Ring", Zone::Hand)));
    assert_eq!(g.pt(bf(&g, 0, "Trinket Mage")), (2, 2));
    // Declining: no search.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Trinket Mage"])
        .library(0, &["Sol Ring", "Island"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    let before = g.player(0).library.clone();
    cast(&mut g, 0, "Trinket Mage");
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.player(0).library, before, "no search, no shuffle");
}

#[test]
fn stoneforge_mystic_finds_an_equipment_and_puts_one_onto_the_battlefield() {
    // "When this creature enters, you may search your library for an
    // Equipment card, reveal it, put it into your hand, then shuffle."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Stoneforge Mystic"])
        .library(0, &["Sol Ring", "Skullclamp", "Lightning Greaves", "Test Bear"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Stoneforge Mystic");
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Yes);
    assert_eq!(offered_defs(&g), defs(&["Skullclamp", "Lightning Greaves"]), "Equipment cards");
    g.choose(Choice::Def(def("Lightning Greaves")));
    settle(&mut g);
    assert!(g.knows(1, g.card(0, "Lightning Greaves", Zone::Hand)));
    // "{1}{W}, {T}: You may put an Equipment card from your hand onto the
    // battlefield."
    let mut g = scen()
        .battlefield(0, &["Stoneforge Mystic", "Plains", "Plains"])
        .hand(0, &["Skullclamp", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let sfm = bf(&g, 0, "Stoneforge Mystic");
    assert_eq!(g.pt(sfm), (1, 2));
    g.choose(act(sfm, 0));
    pay(&mut g);
    assert!(g.obj(sfm).tapped);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::YesNo), "you may");
    g.choose(Choice::Yes);
    assert_eq!(g.kind(), Some(DecisionKind::ChooseHandCard));
    let clamp = g.card(0, "Skullclamp", Zone::Hand);
    assert!(!g.has_option(Choice::Card(g.card(0, "Sol Ring", Zone::Hand))), "an Equipment card");
    g.choose(Choice::Card(clamp));
    settle(&mut g);
    assert_eq!(g.obj(clamp).zone, Zone::Battlefield);
}

// ---- Sylvan Safekeeper --------------------------------------------------------------------

#[test]
fn sylvan_safekeeper_sacrifices_a_land_to_give_shroud() {
    // "Sacrifice a land: Target creature you control gains shroud until end
    // of turn."
    let mut g = scen()
        .battlefield(0, &["Sylvan Safekeeper", "Test Bear", "Forest"])
        .battlefield(1, &["Test Reacher", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let ss = bf(&g, 0, "Sylvan Safekeeper");
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(ss), (1, 1));
    g.choose(act(ss, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Test Reacher"))), "you control");
    assert!(g.has_option(g.tgt(ss)), "itself too");
    g.choose(g.tgt(bear));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::Sacrifice));
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(bf(&g, 0, "Forest")));
    }
    assert_eq!(g.count_in(0, "Forest", Zone::Graveyard), 1);
    settle(&mut g);
    assert!(g.has_kw(bear, Kw::Shroud));
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    assert!(!g.has_option(g.tgt(bear)), "shroud");
    // No land, no activation.
    let g = scen().battlefield(0, &["Sylvan Safekeeper"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Sylvan Safekeeper"), 0)));
}

// ---- Titania ------------------------------------------------------------------------------

#[test]
fn titania_returns_a_land_and_makes_an_elemental_when_your_land_dies() {
    // "When Titania enters, return target land card from your graveyard to
    // the battlefield. Whenever a land you control is put into a graveyard
    // from the battlefield, create a 5/3 green Elemental creature token."
    let mut g = scen()
        .battlefield(0, &["Forest"; 5])
        .hand(0, &["Titania, Protector of Argoth"])
        .graveyard(0, &["Wasteland", "Lightning Bolt"])
        .graveyard(1, &["Plains"])
        .battlefield(1, &["Tropical Island"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Titania, Protector of Argoth");
    let wl = g.card(0, "Wasteland", Zone::Graveyard);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(g.card(1, "Plains", Zone::Graveyard))), "your graveyard");
            Some(g.tgt(g.card(0, "Wasteland", Zone::Graveyard)))
        } else {
            None
        }
    });
    assert_eq!(g.obj(wl).zone, Zone::Battlefield);
    let t = bf(&g, 0, "Titania, Protector of Argoth");
    assert_eq!(g.pt(t), (5, 3));
    // Wasteland (sacrificed: your land) destroys P1's Tropical Island (not
    // yours): one Elemental.
    let trop = bf(&g, 1, "Tropical Island");
    g.choose(act(wl, 1));
    g.choose(g.tgt(trop));
    settle(&mut g);
    assert_eq!(g.obj(trop).zone, Zone::Graveyard);
    let els: Vec<CardIdx> = g.battlefield().iter().copied().filter(|&c| g.name(c) == "Elemental").collect();
    assert_eq!(els.len(), 1);
    assert_eq!(g.pt(els[0]), (5, 3));
    assert_eq!(g.colors_of(els[0]), 1 << engine::mana::G);
    assert_eq!(g.obj(els[0]).controller, 0);
}

// ---- Witherbloom Apprentice --------------------------------------------------------------

#[test]
fn witherbloom_apprentice_drains_one_when_you_cast_an_instant_or_sorcery() {
    // "Magecraft - Whenever you cast or copy an instant or sorcery spell,
    // each opponent loses 1 life and you gain 1 life."
    let mut g = scen()
        .battlefield(0, &["Witherbloom Apprentice", "Mountain", "Forest", "Forest"])
        .hand(0, &["Lightning Bolt", "Test Bear"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let wa = bf(&g, 0, "Witherbloom Apprentice");
    assert_eq!(g.pt(wa), (2, 2));
    cast(&mut g, 0, "Lightning Bolt");
    g.choose(player(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!((g.life(0), g.life(1)), (21, 16));
    cast(&mut g, 0, "Test Bear");
    settle(&mut g);
    assert_eq!((g.life(0), g.life(1)), (21, 16), "a creature spell");
    // The opponent's instant doesn't trigger it.
    g.pass();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!((g.life(0), g.life(1)), (18, 16));
}
