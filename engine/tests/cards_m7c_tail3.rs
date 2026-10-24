//! M7c, the single-card tail part 3 (ENGINE.md 30): the cards that needed
//! three or more features, and the face-down cards. Test names cite the CR.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    }).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

#[allow(dead_code)]
fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Pay with the first source offered until the payment is over.
fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
    }
}

/// Run the stack down; `answer` answers what is asked (None: first option).
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
    panic!("resolve_with: no progress\n{}", log(g));
}

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Cast `name` from P0's hand announcing X (an X decision first), then pay.
fn cast_x(g: &mut Game, name: &str, x: u8) -> CardIdx {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(x));
    pay(g);
    c
}

// ---- Vicious Rivalry, Toxic Deluge (X life) ---------------------------------

/// "As an additional cost to cast this spell, pay X life. Destroy all
/// artifacts and creatures with mana value X or less" (CR 107.3, 601.2b).
#[test]
fn vicious_rivalry_pays_x_life_and_destroys_mana_value_x_or_less_cr107_3() {
    let mut g = scen()
        .battlefield(0, &["Bayou", "Bayou", "Bayou", "Bayou", "Test Bear", "Sol Ring"])
        .hand(0, &["Vicious Rivalry"])
        .battlefield(1, &["Test Giant", "Mind Stone", "Test Flyer", "Forest"])
        .at(Step::Main1, 0)
        .start();
    cast_x(&mut g, "Vicious Rivalry", 2);
    settle(&mut g);
    assert_eq!(g.life(0), 18, "X life paid\n{}", log(&g));
    for (p, n) in [(0, "Test Bear"), (0, "Sol Ring"), (1, "Mind Stone"), (1, "Test Flyer")] {
        assert_eq!(g.count_in(p, n, Zone::Graveyard), 1, "{n} destroyed\n{}", log(&g));
    }
    assert_eq!(g.count_in(1, "Test Giant", Zone::Battlefield), 1, "mana value 5 stays");
    assert_eq!(g.count_in(1, "Forest", Zone::Battlefield), 1, "a land isn't an artifact or creature");
}

/// "As an additional cost to cast this spell, pay X life. All creatures get
/// -X/-X until end of turn" (X locked in, CR 608.2h).
#[test]
fn toxic_deluge_gives_all_creatures_minus_x_cr107_3() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Test Bear"])
        .hand(0, &["Toxic Deluge"])
        .battlefield(1, &["Test Giant", "Test Flyer", "Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    cast_x(&mut g, "Toxic Deluge", 2);
    settle(&mut g);
    assert_eq!(g.life(0), 18);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Test Flyer", Zone::Graveyard), 1);
    assert_eq!(g.pt(bf(&g, 1, "Test Reacher")), (-1, 1), "1/3 survives as -1/1");
    let giant = bf(&g, 1, "Test Giant");
    assert_eq!(g.pt(giant), (3, 3), "5/5 gets -2/-2");
}

// ---- Winds of Abandon ---------------------------------------------------------

/// Overloaded (CR 702.96): "exile each creature you don't control. For each
/// creature exiled this way, its controller searches their library for a
/// basic land card. Those players put those cards onto the battlefield
/// tapped, then shuffle."
#[test]
fn winds_of_abandon_overloaded_exiles_each_and_its_controller_searches_per_creature_cr702_96() {
    let mut g = scen()
        .battlefield(0, &["Plains"; 6])
        .battlefield(0, &["Test Bear"])
        .hand(0, &["Winds of Abandon"])
        .battlefield(1, &["Test Giant", "Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Winds of Abandon", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    assert_eq!(g.pending_decision().unwrap().player, 1, "their controller searches");
    let forest = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
    g.choose(forest);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::SearchLibrary), "a second basic land for the second creature");
    g.choose(forest);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Giant", Zone::Exile) + g.count_in(1, "Test Flyer", Zone::Exile), 2);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1, "yours stay");
    let lands: Vec<_> = g.permanents_of(1).filter(|&c| g.name(c) == "Forest").collect();
    assert_eq!(lands.len(), 2, "two basic lands\n{}", log(&g));
    assert!(lands.iter().all(|&c| g.obj(c).tapped), "tapped");
}

// ---- Mawloc -------------------------------------------------------------------

/// Ravenous (CR 702.156a) and "it fights up to one target creature an
/// opponent controls. If that creature would die this turn, exile it
/// instead" (701.14, 614).
#[test]
fn mawloc_fights_and_the_creature_that_would_die_is_exiled_cr701_14() {
    let mut g = scen()
        .battlefield(0, &["Taiga", "Taiga", "Taiga"])
        .hand(0, &["Mawloc"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast_x(&mut g, "Mawloc", 1);
    let bear = bf(&g, 1, "Test Bear");
    resolve_with(&mut g, |g| if g.kind() == Some(DecisionKind::Target) { Some(g.tgt(bear)) } else { None });
    let m = bf(&g, 0, "Mawloc");
    assert_eq!(g.pt(m), (3, 3), "one +1/+1 counter");
    assert_eq!(g.obj(m).damage, 2, "the Bear fought back");
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile), 1, "exiled instead of dying\n{}", log(&g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 0);
}

// ---- Fastbond -------------------------------------------------------------------

/// "You may play any number of lands on each of your turns. Whenever you
/// play a land, if it wasn't the first land you played this turn, this
/// enchantment deals 1 damage to you" (CR 305.2, 603.4).
#[test]
fn fastbond_any_number_of_lands_and_a_damage_after_the_first_cr305_2() {
    let mut g = scen()
        .battlefield(0, &["Fastbond"])
        .hand(0, &["Forest", "Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    for k in 0..3 {
        let f = g.card(0, "Forest", Zone::Hand);
        assert!(g.has_option(Choice::Play(f)), "land {k} may be played");
        g.choose(Choice::Play(f));
        settle(&mut g);
    }
    assert_eq!(g.count_in(0, "Forest", Zone::Battlefield), 3);
    assert_eq!(g.life(0), 18, "1 damage for the second and the third\n{}", log(&g));
}

// ---- Pest Infestation -------------------------------------------------------------

/// "Destroy up to X target artifacts and/or enchantments. Create twice X 1/1
/// black and green Pest creature tokens" (X announced first, CR 601.2b-c).
#[test]
fn pest_infestation_up_to_x_targets_and_twice_x_pests_cr601_2c() {
    let mut g = scen()
        .battlefield(0, &["Forest"; 5])
        .hand(0, &["Pest Infestation"])
        .battlefield(1, &["Sol Ring", "Mind Stone", "Fastbond"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Pest Infestation", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(g.tgt(ring));
    // Up to X: one is enough.
    assert!(g.has_option(Choice::Pass));
    g.choose(Choice::Pass);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Mind Stone", Zone::Battlefield), 1);
    let pests: Vec<_> = g.permanents_of(0).filter(|&c| g.name(c) == "Pest").collect();
    assert_eq!(pests.len(), 4, "twice X\n{}", log(&g));
    assert_eq!(g.pt(pests[0]), (1, 1));
}

/// "When this artifact is put into a graveyard from the battlefield, draw a
/// card": the IR's event is `leaves_battlefield` (as Chromatic Star's);
/// blocked (docs/ir_defects.md 15).
#[test]
fn wizards_rockets_is_blocked_by_its_ir_event() {
    let err = engine::db::CardDb::cube().expect_id("Wizard's Rockets").unwrap_err();
    assert!(err.contains("IR:TriggeredAbility.event"), "{err}");
}
