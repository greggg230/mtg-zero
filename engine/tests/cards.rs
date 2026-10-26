//! Real cube cards through the IR: burn, counters, "unless pays", Brainstorm's
//! mid-resolution choices, rituals, random discard, extra turns, filters,
//! mana-source decisions.

mod common;

use common::db;
use engine::db::{types, AbilitySlot};
use engine::hidden::{key_kind, key_payload, key_player, KEY_DRAW, KEY_REVEAL};
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Target, Zone};

const LIB: &[&str] = &["Island"; 6];

/// Decisions are not auto-answered here: tests drive forced ones with
/// `drive_until` so they stop exactly where they look.
fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, LIB).library(1, LIB)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

/// Cast from hand; forced decisions (single target, single mana class) are
/// answered; stops at the next real decision or the caster's priority.
fn cast(g: &mut Game, p: u8, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
}

/// Pass until the stack is empty.
fn settle(g: &mut Game) -> Vec<u64> {
    g.drive_until(|g| g.stack().is_empty() && at_priority(g))
}

/// P0 casts a Test Bear; P1 answers with `counter`; both pass, so the
/// counterspell starts resolving.
fn bear_vs(counter: &str, p0_extra: &[&str]) -> Game {
    let mut lands = vec!["Forest", "Mountain"];
    lands.extend_from_slice(p0_extra);
    let mut g = scen()
        .battlefield(0, &lands)
        .hand(0, &["Test Bear"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &[counter])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Bear");
    g.pass(); // P0
    cast(&mut g, 1, counter);
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    g
}

#[test]
fn mana_leak_controller_may_pay() {
    // Pays: the Bear resolves.
    let mut g = bear_vs("Mana Leak", &["Mountain", "Mountain", "Mountain"]);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(0)));
    g.choose(Choice::Yes);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1);
    assert_eq!(g.count_in(1, "Mana Leak", Zone::Graveyard), 1);
    assert!(g.permanents_of(0).filter(|&c| g.is_type(c, types::LAND)).all(|c| g.obj(c).tapped));

    // Declines: countered.
    let mut g = bear_vs("Mana Leak", &["Mountain", "Mountain", "Mountain"]);
    g.choose(Choice::Pass);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);

    // Cannot pay: "yes" is not offered.
    let g = bear_vs("Mana Leak", &["Mountain", "Mountain"]);
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    assert_eq!(g.options_desc(), vec!["0=pass".to_string()]);
}

#[test]
fn memory_lapse_puts_the_spell_on_top_of_its_library() {
    let mut g = bear_vs("Memory Lapse", &[]);
    settle(&mut g);
    let bear = g.card(0, "Test Bear", Zone::Library);
    assert_eq!(g.player(0).library.last(), Some(&bear));
    // Both players saw it go there.
    assert!(g.knows(1, bear) && g.knows(0, bear));
}

#[test]
fn counterspell_counters() {
    let mut g = bear_vs("Counterspell", &[]);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Counterspell", Zone::Graveyard), 1);
}

#[test]
fn spell_pierce_targets_only_noncreature_spells() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Mountain", "Mountain"])
        .hand(0, &["Test Bear", "Lightning Bolt"])
        .battlefield(1, &["Island"])
        .hand(1, &["Spell Pierce"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Bear");
    g.pass(); // P0 passes; P1 has priority
    let sp = g.card(1, "Spell Pierce", Zone::Hand);
    assert_eq!(g.to_move(), Some(1));
    assert!(!g.has_option(Choice::Cast(sp)), "a creature spell is not a legal target");
    settle(&mut g);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    assert!(g.has_option(Choice::Cast(sp)));
}

#[test]
fn brainstorm_draws_three_then_puts_two_back_in_chosen_order() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island"])
        .hand(0, &["Brainstorm", "Test Bear", "Mountain"])
        .library(0, &["Forest", "Plains", "Swamp", "Island"])
        .library(1, LIB)
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Brainstorm");
    // Resolution draws three, then suspends on the first choice.
    let keys = g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    let draws: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_DRAW).collect();
    assert_eq!(draws.len(), 3);
    assert!(draws.iter().all(|&k| key_player(k) == 0));
    assert_eq!(key_payload(draws[0]), db().id("Forest").unwrap() as u64);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseHandCard), Some(0)));
    assert_eq!(g.pending_decision().unwrap().options.len(), 5, "one option per distinct card");
    // A clone taken mid-resolution resumes identically (ENGINE.md 4).
    let mut copy = g.clone();
    let mountain = g.card(0, "Mountain", Zone::Hand);
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Card(mountain));
    copy.choose(Choice::Card(mountain));
    g.choose(Choice::Card(bear));
    copy.choose(Choice::Card(bear));
    assert_eq!(g.fingerprint(), copy.fingerprint());
    let lib = &g.player(0).library;
    assert_eq!(lib[lib.len() - 1], bear, "the last card chosen is on top");
    assert_eq!(lib[lib.len() - 2], mountain);
    assert!(g.knows(0, bear) && !g.knows(1, bear));
    assert_eq!(g.player(0).hand.len(), 3);
}

#[test]
fn ancestral_recall_targets_a_player() {
    let mut g = scen().battlefield(0, &["Island"]).hand(0, &["Ancestral Recall"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Ancestral Recall", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.choose(Choice::Target(Target::Player(1)));
    settle(&mut g);
    assert_eq!(g.player(1).hand.len(), 3);
}

#[test]
fn dark_ritual_pays_for_hymn_to_tourach_random_discard() {
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Dark Ritual", "Hymn to Tourach"])
        .hand(1, &["Plains", "Mountain", "Forest"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Dark Ritual");
    settle(&mut g);
    assert_eq!(g.player(0).pool.amt[engine::mana::B], 3);
    let hymn = g.card(0, "Hymn to Tourach", Zone::Hand);
    g.choose(Choice::Cast(hymn));
    g.choose(Choice::Target(Target::Player(1)));
    assert_eq!(g.player(0).pool.amt[engine::mana::B], 1);
    let keys = settle(&mut g);
    assert_eq!(g.player(1).hand.len(), 1);
    assert_eq!(g.player(1).graveyard.len(), 2);
    let reveals: Vec<u64> =
        keys.iter().copied().filter(|&k| key_kind(k) == KEY_REVEAL && key_player(k) == 1).collect();
    assert_eq!(reveals.len(), 2, "each random discard is a keyed reveal");
}

#[test]
fn time_walk_takes_an_extra_turn() {
    let mut g = scen().battlefield(0, &["Island", "Mountain"]).hand(0, &["Time Walk"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Time Walk");
    g.drive_until(|g| g.turn().number == 4);
    assert_eq!(g.turn().active, 0);
    g.drive_until(|g| g.turn().number == 5);
    assert_eq!(g.turn().active, 1);
}

#[test]
fn cut_down_and_flame_slash_filters() {
    // ruling: 94ad0c21
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain"])
        .hand(0, &["Cut Down", "Flame Slash"])
        .battlefield(1, &["Test Bear", "Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let cd = g.card(0, "Cut Down", Zone::Hand);
    g.choose(Choice::Cast(cd));
    // Only the Bear has total power and toughness 5 or less.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert_eq!(g.options_desc().len(), 1);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    cast(&mut g, 0, "Flame Slash");
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Trampler", Zone::Graveyard), 1);
}

#[test]
fn cogwork_librarian_is_a_vanilla_three_three() {
    let mut g = scen().battlefield(0, &["Island"; 4]).hand(0, &["Cogwork Librarian"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Cogwork Librarian");
    settle(&mut g);
    let c = g.card(0, "Cogwork Librarian", Zone::Battlefield);
    assert_eq!((g.power(c), g.toughness(c)), (3, 3));
}

#[test]
fn mana_sources_are_chosen_by_class_in_canonical_order() {
    let mut g = scen()
        .battlefield(0, &["Island", "Plains", "Tundra"])
        .hand(0, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let f = g.card(0, "Test Flyer", Zone::Hand);
    g.choose(Choice::Cast(f)); // {1}{U}
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    let island = g.card(0, "Island", Zone::Battlefield);
    let plains = g.card(0, "Plains", Zone::Battlefield);
    let tundra = g.card(0, "Tundra", Zone::Battlefield);
    let tap = |c| Choice::Ability(c, AbilitySlot::Activated(0));
    // Tundra first would leave {1} for the classes after it: none.
    assert!(g.has_option(tap(island)) && g.has_option(tap(plains)));
    assert!(!g.has_option(tap(tundra)));
    g.choose(tap(plains));
    // Then only the Tundra can make the {U}.
    assert_eq!(g.options_desc().len(), 1);
    g.drive_until(at_priority);
    assert!(g.obj(plains).tapped && g.obj(tundra).tapped && !g.obj(island).tapped);
}
