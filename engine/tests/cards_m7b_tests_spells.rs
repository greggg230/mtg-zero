//! M7b card tests, instants and sorceries (and Touch the Spirit Realm):
//! Boomerang Basics, Crop Rotation, Dig Through Time, Echo of Eons,
//! Enlightened Tutor, Erode, Exorcise, Flashback, Force of Vigor, Imperial
//! Seal, Kolaghan's Command, Mana Tithe, Mine Collapse, Natural Order,
//! Reprieve, Stern Scolding, Touch the Spirit Realm, Unearth, Vindicate,
//! Worldly Tutor; each against its Oracle text.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
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

fn pay(g: &mut Game) {
    let paying = |g: &Game| {
        matches!(
            g.kind(),
            Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana) | Some(DecisionKind::ChooseGraveyardCard)
        )
    };
    g.drive_until(|g| at_priority(g) || !paying(g));
    while paying(g) {
        let d = g.pending_decision().unwrap();
        let c = d.options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass).unwrap_or(d.options[0].choice);
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !paying(g));
    }
}

/// Cast from hand; stops at the first decision that isn't forced (a target,
/// a mode, an alternative cost) or after paying.
fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(g);
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

fn modes(g: &Game) -> Vec<u8> {
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::ChooseMode, "{:?}", g.options_desc());
    d.options.iter().filter_map(|o| if let Choice::Number(k) = o.choice { Some(k) } else { None }).collect()
}

// ---- tutors ---------------------------------------------------------------------

/// Enlightened Tutor ("an artifact or enchantment card"), Worldly Tutor ("a
/// creature card"): reveal it, then shuffle and put it on top. Imperial Seal
/// ("a card", not revealed; you lose 2 life).
#[test]
fn tutors_put_the_found_card_on_top() {
    const LIB: &[&str] = &["Lightning Bolt", "Sol Ring", "Oath of Druids", "Test Bear", "Island", "Swamp"];
    let rows: &[(&str, &str, &[&str], &str, bool, i32)] = &[
        ("Enlightened Tutor", "Plains", &["Sol Ring", "Oath of Druids"], "Oath of Druids", true, 20),
        ("Worldly Tutor", "Forest", &["Test Bear"], "Test Bear", true, 20),
        (
            "Imperial Seal",
            "Swamp",
            &["Lightning Bolt", "Sol Ring", "Oath of Druids", "Test Bear", "Island", "Swamp"],
            "Sol Ring",
            false,
            18,
        ),
    ];
    for &(tutor, land, offered, pick, revealed, life) in rows {
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .battlefield(0, &[land])
            .hand(0, &[tutor])
            .library(0, LIB)
            .library(1, &["Forest"; 5])
            .at(Step::Main1, 0)
            .start();
        let t = g.card(0, tutor, Zone::Hand);
        cast(&mut g, 0, tutor);
        g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
        assert_eq!(offered_defs(&g), defs(offered), "{tutor}");
        assert_eq!(g.has_option(Choice::Pass), revealed, "{tutor}: a stated quality may be missed (701.19b)");
        g.choose(Choice::Def(def(pick)));
        settle(&mut g);
        let top = *g.player(0).library.last().unwrap();
        assert_eq!(g.name(top), pick, "{tutor}: on top");
        assert_eq!(g.knows(1, top), revealed, "{tutor}: revealed or not");
        assert!(g.knows(0, top));
        assert_eq!(g.life(0), life as i64, "{tutor}");
        assert_eq!(g.obj(t).zone, Zone::Graveyard);
        assert_eq!(g.player(0).library.len(), LIB.len());
    }
    // Worldly and Enlightened are instants, Imperial Seal a sorcery.
    let g = scen()
        .battlefield(0, &["Forest", "Swamp"])
        .hand(0, &["Worldly Tutor", "Imperial Seal"])
        .at(Step::Upkeep, 0)
        .start();
    assert!(g.has_option(Choice::Cast(g.card(0, "Worldly Tutor", Zone::Hand))));
    assert!(!g.has_option(Choice::Cast(g.card(0, "Imperial Seal", Zone::Hand))));
}

#[test]
fn natural_order_sacrifices_a_green_creature_for_a_green_creature_card() {
    // "As an additional cost to cast this spell, sacrifice a green creature.
    // Search your library for a green creature card, put it onto the
    // battlefield, then shuffle."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Test Bear", "Test Flyer"])
        .hand(0, &["Natural Order"])
        .library(0, &["Test Giant", "Test Flyer", "Uro, Titan of Nature's Wrath", "Island"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let no = g.card(0, "Natural Order", Zone::Hand);
    g.choose(Choice::Cast(no));
    g.drive_until(|g| {
        at_priority(g) || matches!(g.kind(), Some(DecisionKind::Sacrifice) | Some(DecisionKind::PayMana))
    });
    pay(&mut g);
    if g.kind() == Some(DecisionKind::Sacrifice) {
        assert!(!g.has_option(Choice::Card(bf(&g, 0, "Test Flyer"))), "a green creature: the Flyer is blue");
        g.choose(Choice::Card(bear));
    }
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(offered_defs(&g), defs(&["Test Giant", "Uro, Titan of Nature's Wrath"]), "green creature cards");
    g.choose(Choice::Def(def("Test Giant")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Giant", Zone::Battlefield), 1);
    // Without a green creature it can't be cast.
    let g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Test Flyer"])
        .hand(0, &["Natural Order"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Natural Order", Zone::Hand))));
}

#[test]
fn cr_601_2h_natural_order_may_sacrifice_the_elf_that_paid_for_it() {
    // 601.2g-h: mana abilities are activated before the costs are paid, so
    // a green creature that tapped for mana can then be sacrificed.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Forest", "Forest", "Forest", "Elvish Mystic"])
        .hand(0, &["Natural Order"])
        .library(0, &["Test Giant", "Island"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    let no = g.card(0, "Natural Order", Zone::Hand);
    assert!(g.has_option(Choice::Cast(no)), "Elvish Mystic pays {{G}} and is then sacrificed");
    g.choose(Choice::Cast(no));
    pay(&mut g);
    g.drive_until(|g| {
        !matches!(g.kind(), Some(DecisionKind::Sacrifice)) || g.has_option(Choice::Card(bf(g, 0, "Elvish Mystic")))
    });
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(bf(&g, 0, "Elvish Mystic")));
    }
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Def(def("Test Giant")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Giant", Zone::Battlefield), 1);
    assert_eq!(g.count_in(0, "Elvish Mystic", Zone::Graveyard), 1);
}

#[test]
fn crop_rotation_sacrifices_a_land_to_put_any_land_card_onto_the_battlefield() {
    // "As an additional cost to cast this spell, sacrifice a land. Search
    // your library for a land card, put that card onto the battlefield,
    // then shuffle."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Forest", "Island"])
        .hand(0, &["Crop Rotation"])
        .library(0, &["Gaea's Cradle", "Lightning Bolt", "Tolarian Academy"])
        .library(1, &["Forest"; 5])
        .at(Step::Upkeep, 0)
        .start();
    let cr = g.card(0, "Crop Rotation", Zone::Hand);
    g.choose(Choice::Cast(cr)); // an instant: in the upkeep
    g.drive_until(|g| {
        at_priority(g) || matches!(g.kind(), Some(DecisionKind::Sacrifice) | Some(DecisionKind::PayMana))
    });
    pay(&mut g);
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(bf(&g, 0, "Island")));
    }
    assert_eq!(g.count_in(0, "Island", Zone::Graveyard), 1);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(offered_defs(&g), defs(&["Gaea's Cradle", "Tolarian Academy"]), "land cards");
    g.choose(Choice::Def(def("Gaea's Cradle")));
    settle(&mut g);
    let cradle = bf(&g, 0, "Gaea's Cradle");
    assert!(!g.obj(cradle).tapped);
}

#[test]
fn cr_601_2h_crop_rotation_may_sacrifice_the_land_that_paid_for_it() {
    // Tap the only Forest for {G}, then sacrifice it (601.2g-h).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Forest"])
        .hand(0, &["Crop Rotation"])
        .library(0, &["Gaea's Cradle", "Island"])
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    let cr = g.card(0, "Crop Rotation", Zone::Hand);
    assert!(g.has_option(Choice::Cast(cr)), "the Forest pays {{G}} and is then sacrificed");
    g.choose(Choice::Cast(cr));
    pay(&mut g);
    if g.kind() == Some(DecisionKind::Sacrifice) {
        g.choose(Choice::Card(bf(&g, 0, "Forest")));
    }
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Def(def("Gaea's Cradle")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Gaea's Cradle", Zone::Battlefield), 1);
    assert_eq!(g.count_in(0, "Forest", Zone::Graveyard), 1);
}

// ---- Dig Through Time, Echo of Eons -------------------------------------------------

#[test]
fn dig_through_time_delves_and_takes_two_of_the_top_seven() {
    // "Delve. Look at the top seven cards of your library. Put two of them
    // into your hand and the rest on the bottom of your library in any
    // order."
    let lib =
        ["Lightning Bolt", "Counterspell", "Sol Ring", "Forest", "Plains", "Swamp", "Mountain", "Island", "Island"];
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Dig Through Time"])
        .graveyard(0, &["Test Bear", "Test Bear", "Test Bear", "Test Bear", "Test Bear", "Test Bear"])
        .library(0, &lib)
        .library(1, &["Forest"; 5])
        .at(Step::Main1, 0)
        .start();
    let dig = g.card(0, "Dig Through Time", Zone::Hand);
    assert!(g.has_option(Choice::Cast(dig)), "{{6}}{{U}}{{U}} with two Islands and six cards to delve");
    cast(&mut g, 0, "Dig Through Time");
    assert_eq!(g.player(0).graveyard.len(), 0, "six cards exiled by delve");
    assert_eq!(g.count_in(0, "Test Bear", Zone::Exile), 6);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand));
    let pick = |g: &mut Game, name: &str| {
        let c = g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .map(|o| o.choice)
            .find(|&c| matches!(c, Choice::Card(x) if g.name(x) == name))
            .unwrap_or_else(|| panic!("{name}: {:?}", g.options_desc()));
        g.choose(c);
    };
    let first = ["Lightning Bolt", "Sol Ring"].into_iter().min_by_key(|n| def(n)).unwrap();
    let second = ["Lightning Bolt", "Sol Ring"].into_iter().max_by_key(|n| def(n)).unwrap();
    pick(&mut g, first);
    pick(&mut g, second);
    // The other five go to the bottom (one decision per card but the last).
    let mut placed = 0;
    while g.kind() == Some(DecisionKind::PutOnBottom) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        placed += 1;
    }
    assert!(placed >= 4, "any order");
    settle(&mut g);
    let hand: Vec<&str> = g.player(0).hand.iter().map(|&c| g.name(c)).collect();
    assert!(hand.contains(&"Lightning Bolt") && hand.contains(&"Sol Ring") && hand.len() == 2, "{hand:?}");
    let lib0 = &g.player(0).library;
    assert_eq!(lib0.len(), 7);
    let mut bottom: Vec<&str> = lib0[..5].iter().map(|&c| g.name(c)).collect();
    bottom.sort();
    assert_eq!(bottom, ["Counterspell", "Forest", "Mountain", "Plains", "Swamp"]);
    assert_eq!(g.obj(dig).zone, Zone::Graveyard);
}

#[test]
fn echo_of_eons_flashback_each_player_shuffles_hand_and_graveyard_into_library_draws_seven() {
    // "Each player shuffles their hand and graveyard into their library,
    // then draws seven cards. Flashback {2}{U}."
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt", "Sol Ring"])
        .graveyard(0, &["Echo of Eons", "Test Bear", "Counterspell"])
        .hand(1, &["Forest"])
        .graveyard(1, &["Plains", "Plains", "Plains", "Plains"])
        .at(Step::Main1, 0)
        .start();
    let echo = g.card(0, "Echo of Eons", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(echo)), "flashback {{2}}{{U}} from the graveyard");
    g.choose(Choice::Cast(echo));
    pay(&mut g);
    settle(&mut g);
    for p in 0..2 {
        assert_eq!(g.player(p).hand.len(), 7, "P{p} drew seven");
        assert!(g.player(p).graveyard.is_empty(), "P{p}: graveyard shuffled in");
    }
    // P0: 10 + 2 + 2 - 7; P1: 10 + 1 + 4 - 7.
    assert_eq!(g.player(0).library.len(), 7);
    assert_eq!(g.player(1).library.len(), 8);
    assert_eq!(g.obj(echo).zone, Zone::Exile, "flashback: exiled (702.34a)");
    // Its mana cost {4}{U}{U} from hand.
    let g = scen().battlefield(0, &["Island"; 5]).hand(0, &["Echo of Eons"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Echo of Eons", Zone::Hand))), "five lands: {{4}}{{U}}{{U}} unpaid");
    let g = scen().battlefield(0, &["Island"; 6]).hand(0, &["Echo of Eons"]).at(Step::Main1, 0).start();
    assert!(g.has_option(Choice::Cast(g.card(0, "Echo of Eons", Zone::Hand))));
}

// ---- removal ---------------------------------------------------------------------------

#[test]
fn erode_destroys_a_creature_and_its_controller_may_fetch_a_tapped_basic() {
    // "Destroy target creature or planeswalker. Its controller may search
    // their library for a basic land card, put it onto the battlefield
    // tapped, then shuffle."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Plains"])
        .hand(0, &["Erode"])
        .battlefield(1, &["Test Bear", "Sol Ring"])
        .library(0, &["Island"; 5])
        .library(1, &["Tropical Island", "Swamp", "Island"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let erode = g.card(0, "Erode", Zone::Hand);
    g.choose(Choice::Cast(erode));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Sol Ring"))), "a creature or planeswalker");
    g.choose(g.tgt(bear));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(1)), "its controller may");
    g.choose(Choice::Yes);
    assert_eq!(offered_defs(&g), defs(&["Swamp", "Island"]), "basic land cards");
    g.choose(Choice::Def(def("Swamp")));
    settle(&mut g);
    assert!(g.obj(bf(&g, 1, "Swamp")).tapped, "tapped");
}

#[test]
fn exorcise_exiles_an_artifact_an_enchantment_or_a_big_creature() {
    // "Exile target artifact, enchantment, or creature with power 4 or
    // greater." A sorcery.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Exorcise"])
        .battlefield(1, &["Test Bear", "Test Trampler", "Sol Ring", "Oath of Druids"])
        .at(Step::Main1, 0)
        .start();
    let ex = g.card(0, "Exorcise", Zone::Hand);
    g.choose(Choice::Cast(ex));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Test Bear"))), "power 2");
    assert!(g.has_option(g.tgt(bf(&g, 1, "Sol Ring"))) && g.has_option(g.tgt(bf(&g, 1, "Oath of Druids"))));
    let tr = bf(&g, 1, "Test Trampler");
    g.choose(g.tgt(tr));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(tr).zone, Zone::Exile);
    let g = scen()
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Exorcise"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Upkeep, 0)
        .start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Exorcise", Zone::Hand))), "a sorcery");
}

#[test]
fn vindicate_destroys_any_permanent_a_land_too() {
    // "Destroy target permanent." A sorcery.
    let mut g = scen()
        .battlefield(0, &["Plains", "Swamp", "Island"])
        .hand(0, &["Vindicate"])
        .battlefield(1, &["Tropical Island", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let v = g.card(0, "Vindicate", Zone::Hand);
    g.choose(Choice::Cast(v));
    assert!(g.has_option(g.tgt(bf(&g, 1, "Test Bear"))));
    assert!(!g.has_option(player(1)), "a permanent, not a player");
    let trop = bf(&g, 1, "Tropical Island");
    g.choose(g.tgt(trop));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(trop).zone, Zone::Graveyard);
}

#[test]
fn boomerang_basics_bounces_a_nonland_permanent_and_draws_if_it_was_yours() {
    // "Return target nonland permanent to its owner's hand. If you
    // controlled that permanent, draw a card."
    let mut g = scen()
        .battlefield(0, &["Island", "Sol Ring"])
        .hand(0, &["Boomerang Basics"])
        .battlefield(1, &["Test Bear", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Boomerang Basics", Zone::Hand);
    g.choose(Choice::Cast(bb));
    assert!(!g.has_option(g.tgt(bf(&g, 1, "Forest"))), "nonland");
    let ring = bf(&g, 0, "Sol Ring");
    g.choose(g.tgt(ring));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(ring).zone, Zone::Hand);
    assert_eq!(g.player(0).hand.len(), 2, "Sol Ring and a drawn card");
    // The opponent's creature: no draw.
    let mut g = scen()
        .battlefield(0, &["Island"])
        .hand(0, &["Boomerang Basics"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Boomerang Basics", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Cast(bb));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(bear));
    }
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    assert_eq!(g.player(1).hand.len(), 1);
    assert_eq!(g.player(0).hand.len(), 0, "you didn't control it: no draw");
}

#[test]
fn mine_collapse_sacrifices_a_mountain_on_your_turn_for_five_damage() {
    // "If it's your turn, you may sacrifice a Mountain rather than pay this
    // spell's mana cost. Mine Collapse deals 5 damage to target creature or
    // planeswalker."
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Mine Collapse"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let mc = g.card(0, "Mine Collapse", Zone::Hand);
    let giant = bf(&g, 1, "Test Giant");
    assert!(g.has_option(Choice::Cast(mc)), "your turn: sacrifice a Mountain");
    g.choose(Choice::Cast(mc));
    for _ in 0..10 {
        match g.kind() {
            Some(DecisionKind::ChooseAltCost) => {
                g.choose(Choice::Number(1));
            }
            Some(DecisionKind::Target) => {
                assert!(!g.has_option(player(1)), "a creature or planeswalker");
                g.choose(g.tgt(giant));
            }
            Some(DecisionKind::Sacrifice) => {
                g.choose(Choice::Card(bf(&g, 0, "Mountain")));
            }
            _ => break,
        }
    }
    assert_eq!(g.count_in(0, "Mountain", Zone::Graveyard), 1);
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "5 damage to a 5/5");
    // Not your turn: only the mana cost ({3}{R}).
    let g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Mine Collapse"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Upkeep, 1)
        .start();
    let mut g = g;
    g.pass();
    assert_eq!(g.to_move(), Some(0));
    assert!(!g.has_option(Choice::Cast(g.card(0, "Mine Collapse", Zone::Hand))), "the opponent's turn");
    // A Mountain is needed: Taiga is one.
    let g = scen()
        .battlefield(0, &["Taiga"])
        .hand(0, &["Mine Collapse"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    assert!(g.has_option(Choice::Cast(g.card(0, "Mine Collapse", Zone::Hand))));
    let g = scen()
        .battlefield(0, &["Forest"])
        .hand(0, &["Mine Collapse"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Mine Collapse", Zone::Hand))));
}

#[test]
fn force_of_vigor_exiles_a_green_card_on_the_opponents_turn_to_destroy_two() {
    // "If it's not your turn, you may exile a green card from your hand
    // rather than pay this spell's mana cost. Destroy up to two target
    // artifacts and/or enchantments."
    let mut g = scen()
        .hand(0, &["Force of Vigor", "Test Bear", "Lightning Bolt"])
        .battlefield(1, &["Sol Ring", "Oath of Druids", "Mox Jet", "Test Hasty"])
        .at(Step::Upkeep, 1)
        .start();
    g.pass();
    let fov = g.card(0, "Force of Vigor", Zone::Hand);
    assert!(g.has_option(Choice::Cast(fov)), "not your turn");
    g.choose(Choice::Cast(fov));
    let ring = bf(&g, 1, "Sol Ring");
    let oath = bf(&g, 1, "Oath of Druids");
    let bear = g.card(0, "Test Bear", Zone::Hand);
    let mut targets = 0;
    for _ in 0..10 {
        match g.kind() {
            Some(DecisionKind::ChooseAltCost) => {
                g.choose(Choice::Number(1));
            }
            Some(DecisionKind::Target) => {
                assert!(!g.has_option(g.tgt(bf(&g, 1, "Test Hasty"))), "artifacts and/or enchantments");
                let t = if targets == 0 { ring } else { oath };
                assert_eq!(g.has_option(Choice::Pass), true, "up to two: zero or one target is allowed");
                g.choose(g.tgt(t));
                targets += 1;
            }
            Some(DecisionKind::ExileCost) => {
                assert!(!g.has_option(Choice::Card(g.card(0, "Lightning Bolt", Zone::Hand))), "a green card");
                g.choose(Choice::Card(bear));
            }
            _ => break,
        }
    }
    assert_eq!(targets, 2, "up to two");
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    settle(&mut g);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert_eq!(g.obj(oath).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Mox Jet", Zone::Battlefield), 1);
    // On your own turn only the mana cost: no lands, no cast.
    let g = scen().hand(0, &["Force of Vigor", "Test Bear"]).battlefield(1, &["Sol Ring"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Force of Vigor", Zone::Hand))), "your turn");
}

#[test]
fn kolaghans_command_chooses_two_different_modes() {
    // ruling: 4ba4f68d
    // "Choose two - Return target creature card from your graveyard to your
    // hand; Target player discards a card; Destroy target artifact;
    // Kolaghan's Command deals 2 damage to any target."
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain"])
        .hand(0, &["Kolaghan's Command"])
        .graveyard(0, &["Test Bear", "Lightning Bolt"])
        .graveyard(1, &["Test Flyer"])
        .battlefield(1, &["Sol Ring", "Test Reacher"])
        .hand(1, &["Island", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let kc = g.card(0, "Kolaghan's Command", Zone::Hand);
    g.choose(Choice::Cast(kc));
    // A set of two modes is chosen in printed order (each set once): the
    // last mode can't come first.
    assert_eq!(modes(&g), vec![0, 1, 2]);
    assert!(!g.has_option(Choice::Pass), "choose two");
    g.choose(Choice::Number(0));
    assert_eq!(modes(&g), vec![1, 2, 3], "two different modes");
    assert!(!g.has_option(Choice::Pass));
    g.choose(Choice::Number(3));
    // Mode 0: a creature card in your graveyard.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(g.card(0, "Lightning Bolt", Zone::Graveyard))));
    assert!(!g.has_option(g.tgt(g.card(1, "Test Flyer", Zone::Graveyard))), "your graveyard");
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    g.choose(g.tgt(bear));
    // Mode 3: any target.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    let reacher = bf(&g, 1, "Test Reacher");
    g.choose(player(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    assert_eq!(g.life(1), 18);
    assert_eq!(g.obj(reacher).damage, 0);
    // Discard + destroy artifact.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain"])
        .hand(0, &["Kolaghan's Command"])
        .battlefield(1, &["Sol Ring"])
        .hand(1, &["Island", "Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let kc = g.card(0, "Kolaghan's Command", Zone::Hand);
    g.choose(Choice::Cast(kc));
    assert_eq!(modes(&g), vec![1, 2], "no creature card in your graveyard: mode 0 can't be chosen");
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(2));
    let ring = bf(&g, 1, "Sol Ring");
    let mut asked_player = false;
    for _ in 0..5 {
        if g.kind() != Some(DecisionKind::Target) {
            break;
        }
        if g.has_option(player(1)) {
            g.choose(player(1));
            asked_player = true;
        } else {
            g.choose(g.tgt(ring));
        }
    }
    assert!(asked_player, "target player");
    pay(&mut g);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Discard) {
            assert_eq!(g.to_move(), Some(1), "the player discards a card of their choice");
            Some(Choice::Card(g.card(1, "Counterspell", Zone::Hand)))
        } else {
            None
        }
    });
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Counterspell", Zone::Graveyard), 1);
    assert_eq!(g.player(1).hand.len(), 1);
}

// ---- counters ----------------------------------------------------------------------

/// P0 casts `spell`; P1 responds with `counter` (and its targets).
fn spell_vs(spell: &str, p0_lands: &[&str], counter: &str) -> Game {
    let mut g = scen()
        .battlefield(0, p0_lands)
        .hand(0, &[spell])
        .battlefield(1, &["Island", "Plains"])
        .hand(1, &[counter])
        .at(Step::Main1, 0)
        .start();
    let s = g.card(0, spell, Zone::Hand);
    g.choose(Choice::Cast(s));
    pay(&mut g);
    g.drive_until(at_priority);
    g.pass();
    assert_eq!(g.to_move(), Some(1));
    g
}

#[test]
fn mana_tithe_counters_unless_its_controller_pays_one() {
    // ruling: 6a98686c
    // "Counter target spell unless its controller pays {1}."
    let mut g = spell_vs("Test Bear", &["Forest", "Forest", "Forest"], "Mana Tithe");
    let mt = g.card(1, "Mana Tithe", Zone::Hand);
    g.choose(Choice::Cast(mt));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(0)));
    g.choose(Choice::Yes);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1, "paid {{1}}");
    let mut g = spell_vs("Test Bear", &["Forest", "Forest"], "Mana Tithe");
    let mt = g.card(1, "Mana Tithe", Zone::Hand);
    g.choose(Choice::Cast(mt));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    assert_eq!(g.options_desc(), vec!["0=pass".to_string()], "can't pay");
    g.pass();
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1, "countered");
}

#[test]
fn stern_scolding_counters_a_creature_spell_with_power_or_toughness_two_or_less() {
    // "Counter target creature spell with power or toughness 2 or less."
    for (creature, lands, ok) in [
        ("Test Bear", vec!["Forest", "Forest"], true),
        ("Test Reacher", vec!["Forest", "Forest"], true),
        ("Test Wall", vec!["Plains", "Plains"], true),
        ("Test Giant", vec!["Forest"; 5], false),
        ("Oliphaunt", vec!["Mountain"; 6], false),
    ] {
        let mut g = spell_vs(creature, &lands, "Stern Scolding");
        let ss = g.card(1, "Stern Scolding", Zone::Hand);
        assert_eq!(g.has_option(Choice::Cast(ss)), ok, "{creature}");
        if ok {
            g.choose(Choice::Cast(ss));
            pay(&mut g);
            settle(&mut g);
            assert_eq!(g.count_in(0, creature, Zone::Graveyard), 1, "{creature} countered");
        }
    }
    // Not a noncreature spell.
    let g = spell_vs("Sol Ring", &["Island"], "Stern Scolding");
    assert!(!g.has_option(Choice::Cast(g.card(1, "Stern Scolding", Zone::Hand))));
}

#[test]
fn reprieve_returns_a_spell_to_its_owners_hand_and_draws() {
    // "Return target spell to its owner's hand. Draw a card."
    let mut g = spell_vs("Test Bear", &["Forest", "Forest"], "Reprieve");
    let r = g.card(1, "Reprieve", Zone::Hand);
    let hand1 = g.player(1).hand.len();
    g.choose(Choice::Cast(r));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Hand), 1, "not countered: returned to hand");
    assert_eq!(g.player(1).hand.len(), hand1, "Reprieve left, one card drawn");
    assert_eq!(g.count_in(1, "Reprieve", Zone::Graveyard), 1);
}

// ---- graveyard ---------------------------------------------------------------------

#[test]
fn flashback_grants_flashback_for_its_mana_cost_to_an_instant_or_sorcery_card() {
    // "Target instant or sorcery card in your graveyard gains flashback
    // until end of turn. The flashback cost is equal to its mana cost."
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Flashback"])
        .graveyard(0, &["Lightning Bolt", "Test Bear"])
        .graveyard(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(bolt)));
    let fb = g.card(0, "Flashback", Zone::Hand);
    g.choose(Choice::Cast(fb));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g.kind() == Some(DecisionKind::Target) {
        assert!(!g.has_option(g.tgt(g.card(0, "Test Bear", Zone::Graveyard))), "an instant or sorcery card");
        assert!(!g.has_option(g.tgt(g.card(1, "Counterspell", Zone::Graveyard))), "your graveyard");
        g.choose(g.tgt(bolt));
    }
    pay(&mut g);
    settle(&mut g);
    assert!(g.has_option(Choice::Cast(bolt)), "flashback {{R}}");
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.life(1), 17);
    assert_eq!(g.obj(bolt).zone, Zone::Exile, "702.34a");
    assert_eq!(g.count_in(0, "Flashback", Zone::Graveyard), 1);
}

#[test]
fn unearth_returns_a_cheap_creature_card_and_cycles() {
    // "Return target creature card with mana value 3 or less from your
    // graveyard to the battlefield. Cycling {2}."
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .hand(0, &["Unearth"])
        .graveyard(0, &["Test Bear", "Test Giant", "Sol Ring"])
        .graveyard(1, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let u = g.card(0, "Unearth", Zone::Hand);
    g.choose(Choice::Cast(u));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(g.card(0, "Test Giant", Zone::Graveyard))), "mana value 5");
    assert!(!g.has_option(g.tgt(g.card(0, "Sol Ring", Zone::Graveyard))), "a creature card");
    assert!(!g.has_option(g.tgt(g.card(1, "Test Flyer", Zone::Graveyard))), "your graveyard");
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.obj(bear).controller, 0);
    // Cycling {2}.
    let mut g = scen().battlefield(0, &["Swamp", "Swamp"]).hand(0, &["Unearth"]).at(Step::Upkeep, 0).start();
    let u = g.card(0, "Unearth", Zone::Hand);
    g.choose(Choice::Ability(u, AbilitySlot::Hand(0)));
    pay(&mut g);
    assert_eq!(g.obj(u).zone, Zone::Graveyard);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
}

// ---- Touch the Spirit Realm ----------------------------------------------------------

#[test]
fn touch_the_spirit_realm_exiles_until_it_leaves() {
    // "When this enchantment enters, exile up to one target artifact or
    // creature until this enchantment leaves the battlefield."
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .hand(0, &["Touch the Spirit Realm"])
        .battlefield(1, &["Test Bear", "Tropical Island", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    cast(&mut g, 0, "Touch the Spirit Realm");
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(bf(g, 1, "Tropical Island"))), "an artifact or creature");
            assert!(g.has_option(Choice::Pass), "up to one");
            Some(g.tgt(bf(g, 1, "Test Bear")))
        } else {
            None
        }
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    // When it leaves (bounced by Boomerang Basics) the Bear returns.
    let mut g2 = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Island"])
        .hand(0, &["Touch the Spirit Realm", "Boomerang Basics"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g2, 1, "Test Bear");
    let tsr_card = g2.card(0, "Touch the Spirit Realm", Zone::Hand);
    g2.choose(Choice::Cast(tsr_card));
    pay(&mut g2);
    resolve_with(&mut g2, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bf(g, 1, "Test Bear"))));
    assert_eq!(g2.obj(bear).zone, Zone::Exile);
    let bb = g2.card(0, "Boomerang Basics", Zone::Hand);
    g2.choose(Choice::Cast(bb));
    g2.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g2.kind() == Some(DecisionKind::Target) {
        let t = bf(&g2, 0, "Touch the Spirit Realm");
        g2.choose(g2.tgt(t));
    }
    pay(&mut g2);
    settle(&mut g2);
    assert_eq!(g2.obj(tsr_card).zone, Zone::Hand);
    let back = bf(&g2, 1, "Test Bear");
    assert_eq!(g2.obj(back).controller, 1, "610.3a: returned under its owner's control");
}

#[test]
fn touch_the_spirit_realm_channel_flickers_until_the_next_end_step() {
    // "Channel - {1}{W}, Discard this card: Exile target artifact or
    // creature. Return it to the battlefield under its owner's control at
    // the beginning of the next end step."
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Touch the Spirit Realm"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let t = g.card(0, "Touch the Spirit Realm", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Ability(t, AbilitySlot::Hand(0)));
    g.drive_until(|g| {
        at_priority(g) || g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana)
    });
    if g.kind() == Some(DecisionKind::Target) {
        assert!(!g.has_option(Choice::Pass), "exactly one target");
        g.choose(g.tgt(bear));
    }
    pay(&mut g);
    assert_eq!(g.obj(t).zone, Zone::Graveyard, "discarded");
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    quiet_until(&mut g, |g| {
        g.turn().step == Step::End
            && g.stack().is_empty()
            && at_priority(g)
            && g.count_in(1, "Test Bear", Zone::Battlefield) == 1
    });
    let back = bf(&g, 1, "Test Bear");
    assert_eq!(g.obj(back).controller, 1);
    assert_eq!(g.turn().number, 3, "this turn's end step");
}
