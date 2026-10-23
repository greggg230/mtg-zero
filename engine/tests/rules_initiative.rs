//! M7b part 3b (ENGINE.md 27.4): the initiative (CR 725) and the Undercity
//! dungeon (309, 701.49): taking the initiative ventures (725.2), the upkeep
//! venture, combat damage taking it, rooms and the choice between them
//! (309.4-309.5), completing the dungeon (309.7, 704.5t, 701.49c), goad
//! (701.38) and hidden information.

mod common;

use common::db;
use engine::db::{Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
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

fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress");
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

/// The library card of the first option that names a basic land (Secret
/// Entrance's search).
fn find_basic(g: &Game) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| {
        options(g).into_iter().find(|c| matches!(c, Choice::Def(_))).unwrap_or(Choice::Pass)
    })
}

#[test]
fn taking_the_initiative_ventures_into_undercity_cr_725_2() {
    // Feywild Caretaker: "When this creature enters, you take the
    // initiative." Then "whenever a player takes the initiative, that
    // player ventures into Undercity" (a triggered ability): Undercity goes
    // to the command zone with the marker on Secret Entrance, whose ability
    // ("search your library for a basic land card, reveal it, put it into
    // your hand, then shuffle") triggers.
    let mut g = scen().battlefield(0, &["Island"; 5]).hand(0, &["Feywild Caretaker"]).start();
    let c = g.card(0, "Feywild Caretaker", Zone::Hand);
    g.choose(Choice::Cast(c));
    finish(&mut g, |_| None);
    let hand = g.player(0).hand.len();
    resolve_with(&mut g, find_basic);
    assert_eq!(g.initiative(), Some(0));
    assert_eq!(g.dungeon(0), (Some(0), 0), "on Secret Entrance");
    assert_eq!(g.dungeon(1), (None, 0));
    assert_eq!(g.command_zone().len(), 1);
    assert_eq!(g.name(g.command_zone()[0]), "Undercity");
    assert_eq!(g.player(0).hand.len(), hand + 1, "an Island found");
}

#[test]
fn the_initiative_ventures_at_its_holders_upkeep_and_rooms_are_chosen_cr_309_5() {
    // From Secret Entrance the marker moves to Forge or Lost Well (a
    // `ChooseRoom` decision: `NUMBER` = room index); Forge: "put two +1/+1
    // counters on target creature".
    let mut g = scen().battlefield(0, &["Test Bear"]).initiative(0).dungeon(0, Some(0), 0).at(Step::Upkeep, 0).start();
    let bear = bf(&g, 0, "Test Bear");
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseRoom));
    assert_eq!(options(&g), vec![Choice::Number(1), Choice::Number(2)]);
    g.choose(Choice::Number(1));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bear)));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 2);
    assert_eq!(g.dungeon(0), (Some(1), 0));
}

#[test]
fn no_venture_at_the_upkeep_of_a_player_without_the_initiative() {
    let mut g = scen().initiative(1).dungeon(1, Some(0), 0).dungeon(0, Some(0), 0).at(Step::Upkeep, 0).start();
    g.drive_until(|g| g.turn().step != Step::Upkeep || !at_priority(g));
    assert_eq!(g.dungeon(0), (Some(0), 0));
    assert_eq!(g.dungeon(1), (Some(0), 0));
}

#[test]
fn combat_damage_to_its_holder_takes_the_initiative_cr_725_2() {
    // "Whenever one or more creatures a player controls deal combat damage
    // to the player who has the initiative, the controller of those
    // creatures takes the initiative" (controlled by the player who had
    // it); then the new holder ventures. The old holder keeps its dungeon.
    let mut g = scen().battlefield(0, &["Test Bear", "Test Hasty"]).initiative(1).dungeon(1, Some(2), 0).start();
    let bear = bf(&g, 0, "Test Bear");
    let hasty = bf(&g, 0, "Test Hasty");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.turn().step == Step::CombatDamage && at_priority(g));
    let trig: Vec<_> = g.stack().iter().filter(|s| s.controller == 1).collect();
    assert_eq!(trig.len(), 1, "one trigger for both creatures, controlled by P1");
    resolve_with(&mut g, find_basic);
    assert_eq!(g.initiative(), Some(0));
    assert_eq!(g.dungeon(0), (Some(0), 0));
    assert_eq!(g.dungeon(1), (Some(2), 0), "P1's dungeon stays");
    assert_eq!(g.life(1), 16);
}

#[test]
fn taking_the_initiative_again_ventures_again_cr_725_2() {
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Test Initiative"]).initiative(0).dungeon(0, Some(0), 0).start();
    let c = g.card(0, "Test Initiative", Zone::Hand);
    g.choose(Choice::Cast(c));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseRoom)).then_some(Choice::Number(2)));
    assert_eq!(g.dungeon(0), (Some(2), 0), "Lost Well");
    assert_eq!(g.initiative(), Some(0));
}

#[test]
fn throne_of_the_dead_three_then_the_dungeon_is_completed_cr_309_7_704_5t() {
    // Archives leads only to Throne of the Dead Three: "Reveal the top ten
    // cards of your library. Put a creature card from among them onto the
    // battlefield with three +1/+1 counters on it. It gains hexproof until
    // your next turn. Then shuffle." The dungeon is removed (completed)
    // once that ability has left the stack.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Test Bear", "Island", "Test Giant", "Island"])
        .library(1, &["Island"; 5])
        .initiative(0)
        .dungeon(0, Some(6), 0)
        .at(Step::Upkeep, 0)
        .start();
    // The venture (no choice: one exit), then the room ability.
    g.drive_until(|g| at_priority(g) && g.stack().first().is_some_and(|s| s.controller == 0) && g.dungeon(0).0 == Some(8));
    assert_eq!(g.dungeon(0), (Some(8), 0), "still there while Throne's ability is on the stack");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutOntoBattlefield) => {
            options(g).into_iter().find(|c| matches!(c, Choice::Card(x) if g.name(*x) == "Test Giant"))
        }
        _ => None,
    });
    let giant = bf(&g, 0, "Test Giant");
    assert_eq!(g.counters_on(giant, Counter::P1P1), 3);
    assert!(g.has_kw(giant, Kw::Hexproof));
    assert_eq!(g.dungeon(0), (None, 1), "completed (704.5t)");
    assert!(g.command_zone().is_empty());
}

#[test]
fn venturing_on_the_bottommost_room_completes_it_and_starts_anew_cr_701_49c() {
    // With Throne's ability on the stack, P0 takes the initiative again: the
    // venture completes Undercity and starts a new one on Secret Entrance;
    // Throne's ability still resolves.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Test Bear", "Island", "Island", "Island", "Forest"])
        .library(1, &["Island"; 5])
        .battlefield(0, &["Island"])
        .hand(0, &["Test Initiative"])
        .initiative(0)
        .dungeon(0, Some(7), 0)
        .at(Step::Upkeep, 0)
        .start();
    g.drive_until(|g| at_priority(g) && g.dungeon(0).0 == Some(8) && !g.stack().is_empty());
    let c = g.card(0, "Test Initiative", Zone::Hand);
    g.choose(Choice::Cast(c));
    finish(&mut g, |_| None);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => find_basic(g),
        Some(DecisionKind::PutOntoBattlefield) => options(g).into_iter().find(|c| matches!(c, Choice::Card(_))),
        _ => None,
    });
    assert_eq!(g.dungeon(0), (Some(0), 1), "completed, and a new one on Secret Entrance");
    assert_eq!(g.counters_on(bf(&g, 0, "Test Bear"), Counter::P1P1), 3, "Throne's ability resolved");
}

#[test]
fn a_dungeon_on_its_bottommost_room_with_no_ability_waiting_is_completed_cr_704_5t() {
    let g = scen().dungeon(0, Some(8), 0).start();
    assert_eq!(g.dungeon(0), (None, 1));
}

#[test]
fn arena_goads_target_creature_cr_701_38a() {
    // Forge leads to Arena: "Goad target creature": until P0's next turn,
    // it attacks each combat if able, and a player other than P0 if able.
    // P0 goads its own creature: it attacks P1 (not P1's planeswalker).
    let mut g = scen()
        .battlefield(0, &["Test Bear"])
        .battlefield(1, &["Oko, Thief of Crowns"])
        .initiative(0)
        .dungeon(0, Some(1), 0)
        .at(Step::Upkeep, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseRoom) => Some(Choice::Number(4)),
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.dungeon(0), (Some(4), 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || g.kind() == Some(DecisionKind::AttackTarget));
    assert!(g.obj(bear).attacking, "it must attack");
    assert_ne!(g.kind(), Some(DecisionKind::AttackTarget), "it attacks the player");
    assert!(!g.has_option(Choice::Card(bear)));
}

#[test]
fn the_initiative_and_a_room_choice_survive_determinize() {
    // The dungeon and the initiative are public; P0's room choice is pending
    // in every world sampled for either player, and the encoding agrees.
    let mut g = scen()
        .hand(0, &["Island", "Forest"])
        .hand(1, &["Mountain", "Plains"])
        .initiative(0)
        .dungeon(0, Some(0), 0)
        .dungeon(1, Some(4), 1)
        .at(Step::Upkeep, 0)
        .start();
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseRoom));
    let n = engine::az::encode::NUM_FEATURES;
    for viewer in [0u8, 1] {
        let mut x0 = vec![0.0; n];
        g.encode_features(viewer, &mut x0);
        for seed in 0..4 {
            let mut w = g.clone();
            let mut rng = engine::rng::Rng::new(seed);
            w.determinize(viewer, &mut rng);
            assert_eq!(w.pending_decision(), g.pending_decision());
            assert_eq!(w.view_of(viewer), g.view_of(viewer));
            let mut x1 = vec![0.0; n];
            w.encode_features(viewer, &mut x1);
            assert!(x0 == x1, "viewer {viewer}: the encoding differs in a sampled world");
        }
    }
}
