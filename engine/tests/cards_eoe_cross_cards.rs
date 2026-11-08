//! The cross-lane cards of the Edge of Eternities port (ENGINE.md 99.7,
//! 99.8): the cards that became supported only when the lanes merged, each
//! ability against its Oracle text. Test names cite the CR; the rulings
//! quoted are the set's (Scryfall), not in docs/rulings.md (the cube's).

mod eoe_x_kit;

use engine::db::{types, Counter, Kw};
use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_x_kit::*;

fn is_creature(g: &Game, c: engine::CardIdx) -> bool {
    g.types_of(c) & types::CREATURE != 0
}

// ---- Broodguard Elite -------------------------------------------------------

/// Broodguard Elite ({X}{G}{G}, 0/0): "This creature enters with X +1/+1
/// counters on it." Cast for its mana cost with X = 2 (CR 107.3, 122.6).
#[test]
fn broodguard_elite_enters_with_x_counters_cr122_6() {
    let mut g = scen().battlefield(0, &["Forest"; 4]).hand(0, &["Broodguard Elite"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Broodguard Elite", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0)); // its mana cost, not warp
    }
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    assert!(!g.has_option(Choice::Number(3)), "X at most 2 with four Forests: {:?}", g.options_desc());
    g.choose(Choice::Number(2));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.counters_on(c, Counter::P1P1), 2);
    assert_eq!((g.power(c), g.toughness(c)), (2, 2));
    assert!(!g.void_active(), "cast for its mana cost: not warped");
}

/// "Warp {X}{G}" (CR 702.185a): cast for {X}{G}, X = 1; exiled at the
/// beginning of the next end step. "When this creature leaves the
/// battlefield, put its counters on target creature you control" (CR
/// 603.10a: it looks back; 608.2h: its counters as it last existed): the
/// target is a creature its controller controls, not an opponent's.
#[test]
fn broodguard_elite_warped_leaves_and_moves_its_counters_cr702_185a() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Cross Bear"])
        .battlefield(1, &["Cross Giant"])
        .hand(0, &["Broodguard Elite"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Broodguard Elite", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    assert!(!g.has_option(Choice::Number(2)), "X at most 1 with two Forests: {:?}", g.options_desc());
    g.choose(Choice::Number(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(c, Counter::P1P1), 1, "{}", log(&g));
    assert!(g.void_active(), "warped");
    let (bear, giant) = (bf(&g, 0, "Cross Bear"), bf(&g, 1, "Cross Giant"));
    to_end_step(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)), "{:?}", g.options_desc());
    assert!(!g.has_option(g.tgt(giant)), "a creature you control");
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Exile, "warp: exiled at the end step\n{}", log(&g));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "{}", log(&g));
}

/// With X = 0 it enters with no counters and dies as a 0/0 (CR 704.5f); its
/// leaves trigger puts nothing on the target.
#[test]
fn broodguard_elite_with_x_zero_dies_and_moves_nothing_cr704_5f() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Cross Bear"])
        .hand(0, &["Broodguard Elite"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Broodguard Elite", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority));
    g.choose(Choice::Number(0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.counters_on(bf(&g, 0, "Cross Bear"), Counter::P1P1), 0);
}

// ---- Embrace Oblivion ----------------------------------------------------------

/// Embrace Oblivion: "As an additional cost to cast this spell, sacrifice an
/// artifact or creature. Destroy target creature or Spacecraft." (CR
/// 601.2b, 601.2h, 701.8a.) Not castable with neither to sacrifice; a
/// noncreature Spacecraft is a legal target, a noncreature artifact isn't.
#[test]
fn embrace_oblivion_sacrifices_and_destroys_a_creature_or_spacecraft_cr701_8a() {
    let g = scen().battlefield(0, &["Swamp", "Cross Totem"]).battlefield(1, &["Cross Bear"]).hand(0, &["Embrace Oblivion"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Embrace Oblivion", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "no artifact or creature to sacrifice");

    let mut g = scen()
        .battlefield(0, &["Swamp", "Cross Trinket", "Cross Robot"])
        .battlefield(1, &["Cross Bear", "Cross Hulk", "Cross Trinket"])
        .hand(0, &["Embrace Oblivion"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Embrace Oblivion", Zone::Hand);
    let (bear, hulk, their_trinket) = (bf(&g, 1, "Cross Bear"), bf(&g, 1, "Cross Hulk"), bf(&g, 1, "Cross Trinket"));
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)) && g.has_option(g.tgt(hulk)), "{:?}", g.options_desc());
    assert!(!g.has_option(g.tgt(their_trinket)), "a noncreature artifact that isn't a Spacecraft");
    g.choose(g.tgt(hulk));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    let (trinket, robot) = (bf(&g, 0, "Cross Trinket"), bf(&g, 0, "Cross Robot"));
    assert!(g.has_option(Choice::Card(trinket)) && g.has_option(Choice::Card(robot)));
    g.choose(Choice::Card(trinket));
    settle(&mut g);
    assert_eq!(g.obj(trinket).zone, Zone::Graveyard, "sacrificed");
    assert_eq!(g.obj(hulk).zone, Zone::Graveyard, "destroyed\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.obj(robot).zone, Zone::Battlefield);
}

// ---- Hardlight Containment -----------------------------------------------------

/// Hardlight Containment: "Enchant artifact you control" (CR 303.4a); "When
/// this Aura enters, exile target creature an opponent controls until this
/// Aura leaves the battlefield" (CR 610.3); when the enchanted artifact
/// creature dies the Aura goes to the graveyard (704.5m) and the creature
/// returns under its owner's control (610.3c).
#[test]
fn hardlight_containment_exiles_until_it_leaves_cr610_3() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Mountain", "Cross Robot", "Cross Bear"])
        .battlefield(1, &["Cross Giant", "Cross Trinket"])
        .hand(0, &["Hardlight Containment", "Cross Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (robot, bear) = (bf(&g, 0, "Cross Robot"), bf(&g, 0, "Cross Bear"));
    let (giant, their_trinket) = (bf(&g, 1, "Cross Giant"), bf(&g, 1, "Cross Trinket"));
    let aura = g.card(0, "Hardlight Containment", Zone::Hand);
    g.choose(Choice::Cast(aura));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(robot)), "{:?}", g.options_desc());
    assert!(!g.has_option(g.tgt(their_trinket)), "an artifact you control");
    assert!(!g.has_option(g.tgt(bear)), "not a nonartifact creature");
    g.choose(g.tgt(robot));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(giant)));
    assert!(!g.has_option(g.tgt(bear)) && !g.has_option(g.tgt(robot)), "a creature an opponent controls");
    g.choose(g.tgt(giant));
    settle(&mut g);
    assert_eq!(g.obj(aura).attached.map(|r| r.idx), Some(robot));
    assert_eq!(g.obj(giant).zone, Zone::Exile, "{}", log(&g));
    // The Robot dies: the Aura is put into the graveyard, the Giant returns.
    let t = g.tgt(robot);
    cast(&mut g, 0, "Cross Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(aura).zone, Zone::Graveyard, "{}", log(&g));
    let giant = g.card(1, "Cross Giant", Zone::Battlefield);
    assert_eq!(g.obj(giant).controller, 1, "under its owner's control");
}

/// If the Aura leaves the battlefield before its enter trigger resolves, the
/// creature isn't exiled at all (CR 610.3b: the event the exile lasts until
/// happened after the ability triggered, before it resolved).
#[test]
fn hardlight_containment_gone_before_its_trigger_resolves_exiles_nothing_cr610_3b() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Mountain", "Cross Robot"])
        .battlefield(1, &["Cross Giant"])
        .hand(0, &["Hardlight Containment", "Cross Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (robot, giant) = (bf(&g, 0, "Cross Robot"), bf(&g, 1, "Cross Giant"));
    let t = g.tgt(robot);
    let aura = cast(&mut g, 0, "Hardlight Containment", &[t]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(giant));
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.stack().len(), 1, "the enter trigger waits on the stack");
    // In response the Robot dies; the Aura goes to the graveyard (704.5m).
    let t = g.tgt(robot);
    cast(&mut g, 0, "Cross Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(aura).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "never exiled
{}", log(&g));
}

/// "Enchanted permanent has ward {1}" (CR 702.21a): P1's Mental Modulation
/// targeting the enchanted artifact is countered unless P1 pays {1}; with
/// both their lands spent on the spell they can't.
#[test]
fn hardlight_containment_grants_ward_1_cr702_21a() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Cross Trinket"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &["Hardlight Containment"])
        .hand(1, &["Mental Modulation"])
        .at(Step::Main1, 0)
        .start();
    let trinket = bf(&g, 0, "Cross Trinket");
    let t = g.tgt(trinket);
    cast(&mut g, 0, "Hardlight Containment", &[t]);
    settle(&mut g);
    to_opponent_priority(&mut g);
    let hand_before = g.player(1).hand.len();
    let t = g.tgt(trinket);
    cast(&mut g, 1, "Mental Modulation", &[t]);
    settle(&mut g);
    assert!(!g.obj(trinket).tapped, "countered by ward\n{}", log(&g));
    assert_eq!(g.count_in(1, "Mental Modulation", Zone::Graveyard), 1);
    assert_eq!(g.player(1).hand.len(), hand_before - 1, "no card drawn");
}

// ---- Hylderblade ----------------------------------------------------------------

/// Hylderblade: "Equipped creature gets +3/+1. ... Equip {4}" (CR 301.5,
/// 702.6a): equip only as a sorcery, onto a creature you control.
#[test]
fn hylderblade_equip_4_gives_plus_3_plus_1_cr702_6a() {
    let mut g = scen()
        .battlefield(0, &["Hylderblade", "Cross Bear", "Swamp", "Swamp", "Swamp", "Swamp"])
        .battlefield(1, &["Cross Giant"])
        .at(Step::Main1, 0)
        .start();
    let (blade, bear, giant) = (bf(&g, 0, "Hylderblade"), bf(&g, 0, "Cross Bear"), bf(&g, 1, "Cross Giant"));
    g.choose(act(blade, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(giant)), "a creature you control");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(blade).attached.map(|r| r.idx), Some(bear), "{}", log(&g));
    assert_eq!((g.power(bear), g.toughness(bear)), (5, 3));
    assert_eq!(untapped(&g, 0), 0, "{{4}}");
}

/// "Void — At the beginning of your end step, if a nonland permanent left the
/// battlefield this turn or a spell was warped this turn, attach this
/// Equipment to target creature you control" (CR 603.4: an intervening "if";
/// 701.3a). Without void it doesn't trigger.
#[test]
fn hylderblade_void_attaches_at_your_end_step_cr603_4() {
    // No void: nothing at the end step.
    let mut g = scen().battlefield(0, &["Hylderblade", "Cross Bear"]).at(Step::Main1, 0).start();
    let (blade, _bear) = (bf(&g, 0, "Hylderblade"), bf(&g, 0, "Cross Bear"));
    to_end_step(&mut g);
    assert!(g.stack().is_empty() && g.kind() == Some(DecisionKind::Priority), "no trigger: {:?}", g.options_desc());
    assert_eq!(g.obj(blade).attached, None);
    // Void (a creature died this turn): it attaches to the Bear.
    let mut g = scen()
        .battlefield(0, &["Hylderblade", "Cross Bear", "Mountain"])
        .battlefield(1, &["Cross Giant"])
        .hand(0, &["Cross Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (blade, bear, giant) = (bf(&g, 0, "Hylderblade"), bf(&g, 0, "Cross Bear"), bf(&g, 1, "Cross Giant"));
    let t = g.tgt(giant);
    cast(&mut g, 0, "Cross Bolt", &[t]);
    settle(&mut g);
    assert!(g.void_active());
    to_end_step(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || (at_priority(g) && !g.stack().is_empty()));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(bear));
    }
    settle(&mut g);
    assert_eq!(g.obj(blade).attached.map(|r| r.idx), Some(bear), "{}", log(&g));
    assert_eq!((g.power(bear), g.toughness(bear)), (5, 3));
}

// ---- Mental Modulation -----------------------------------------------------------

/// Mental Modulation: "This spell costs {1} less to cast during your turn.
/// Tap target artifact or creature. Draw a card." (CR 601.2f, 701.26a.) In
/// its controller's turn {U} casts it; the target is an artifact or a
/// creature, not a land or an enchantment.
#[test]
fn mental_modulation_costs_1_less_during_your_turn_taps_and_draws_cr601_2f() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .battlefield(1, &["Cross Trinket", "Cross Totem", "Forest", "Cross Bear"])
        .hand(0, &["Mental Modulation"])
        .at(Step::Main1, 0)
        .start();
    let (trinket, totem, forest, bear) =
        (bf(&g, 1, "Cross Trinket"), bf(&g, 1, "Cross Totem"), bf(&g, 1, "Forest"), bf(&g, 1, "Cross Bear"));
    let c = g.card(0, "Mental Modulation", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{{U}} in your turn: {:?}", g.options_desc());
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(trinket)) && g.has_option(g.tgt(bear)));
    assert!(!g.has_option(g.tgt(totem)) && !g.has_option(g.tgt(forest)));
    g.choose(g.tgt(trinket));
    pay(&mut g);
    let hand = g.player(0).hand.len();
    settle(&mut g);
    assert!(g.obj(trinket).tapped, "{}", log(&g));
    assert_eq!(g.player(0).hand.len(), hand + 1, "drew a card");
}

/// In an opponent's turn it costs {1}{U}: one Island isn't enough, two are.
#[test]
fn mental_modulation_costs_full_in_an_opponents_turn_cr601_2f() {
    let mut g = scen().battlefield(0, &["Island"]).battlefield(1, &["Cross Bear"]).hand(0, &["Mental Modulation"]).at(Step::Main1, 1).start();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let c = g.card(0, "Mental Modulation", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "{{1}}{{U}} with one Island");
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Cross Bear"])
        .hand(0, &["Mental Modulation"])
        .at(Step::Main1, 1)
        .start();
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    let c = g.card(0, "Mental Modulation", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{:?}", g.options_desc());
    let bear = bf(&g, 1, "Cross Bear");
    let t = g.tgt(bear);
    cast(&mut g, 0, "Mental Modulation", &[t]);
    assert_eq!(untapped(&g, 0), 0, "{{1}}{{U}}");
    settle(&mut g);
    assert!(g.obj(bear).tapped);
}

// ---- Pinnacle Kill-Ship -----------------------------------------------------------

/// Pinnacle Kill-Ship: "When this Spacecraft enters, it deals 10 damage to up
/// to one target creature" (CR 601.2c, 115.1: "up to one", zero targets
/// allowed; 120.3).
#[test]
fn pinnacle_kill_ship_deals_10_to_up_to_one_target_creature_cr601_2c() {
    let mut g = scen().battlefield(0, &["Island"; 7]).battlefield(1, &["Cross Giant"]).hand(0, &["Pinnacle Kill-Ship"]).at(Step::Main1, 0).start();
    let giant = bf(&g, 1, "Cross Giant");
    cast(&mut g, 0, "Pinnacle Kill-Ship", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(Choice::Pass), "up to one: no target is allowed");
    assert!(!g.has_option(player(1)), "a creature, not a player");
    g.choose(g.tgt(giant));
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Graveyard, "{}", log(&g));
    // No target: nothing happens.
    let mut g = scen().battlefield(0, &["Island"; 7]).battlefield(1, &["Cross Giant"]).hand(0, &["Pinnacle Kill-Ship"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Pinnacle Kill-Ship", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Pass);
    settle(&mut g);
    let giant = bf(&g, 1, "Cross Giant");
    assert_eq!(g.objs()[giant.i()].damage, 0);
    assert_eq!(g.count_in(0, "Pinnacle Kill-Ship", Zone::Battlefield), 1);
}

/// "Station ... It's an artifact creature at 7+. 7+ | Flying" [7/7] (CR
/// 702.184a, 722.2a-b): not a creature below 7 charge counters.
#[test]
fn pinnacle_kill_ship_station_7_is_a_7_7_flyer_cr722_2b() {
    let mut g = scen()
        .battlefield(0, &["Pinnacle Kill-Ship", "Cross Giant", "Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    let (ship, giant, bear) = (bf(&g, 0, "Pinnacle Kill-Ship"), bf(&g, 0, "Cross Giant"), bf(&g, 0, "Cross Bear"));
    station(&mut g, ship, giant);
    assert_eq!(g.counters_on(ship, charge()), 5);
    assert!(!is_creature(&g, ship));
    station(&mut g, ship, bear);
    assert_eq!(g.counters_on(ship, charge()), 7);
    assert!(is_creature(&g, ship), "{}", log(&g));
    assert_eq!((g.power(ship), g.toughness(ship)), (7, 7));
    assert!(g.has_kw(ship, Kw::Flying));
}

// ---- Reroute Systems ----------------------------------------------------------------

/// Reroute Systems, first mode: "Target artifact or creature gains
/// indestructible until end of turn" (CR 700.2, 702.12b): the Bolt's lethal
/// damage doesn't destroy it.
#[test]
fn reroute_systems_mode_one_indestructible_cr702_12b() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Mountain", "Cross Bear", "Cross Trinket"])
        .battlefield(1, &["Forest"])
        .hand(0, &["Reroute Systems", "Cross Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (bear, trinket, forest) = (bf(&g, 0, "Cross Bear"), bf(&g, 0, "Cross Trinket"), bf(&g, 1, "Forest"));
    let c = g.card(0, "Reroute Systems", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)) && g.has_option(g.tgt(trinket)));
    assert!(!g.has_option(g.tgt(forest)), "not a land");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert!(g.has_kw(bear, Kw::Indestructible), "{}", log(&g));
    let t = g.tgt(bear);
    cast(&mut g, 0, "Cross Bolt", &[t]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "{}", log(&g));
}

/// Second mode: "Reroute Systems deals 2 damage to target tapped creature"
/// (CR 700.2, 120.3): an untapped creature isn't a legal target.
#[test]
fn reroute_systems_mode_two_two_damage_to_a_tapped_creature_cr120_3() {
    let mut g = scen()
        .battlefield(0, &["Plains"])
        .battlefield(1, &["Cross Bear", "Cross Giant"])
        .hand(0, &["Reroute Systems"])
        .at(Step::Main1, 0)
        .start();
    let (bear, giant) = (bf(&g, 1, "Cross Bear"), bf(&g, 1, "Cross Giant"));
    g.set_tapped(bear, true);
    let c = g.card(0, "Reroute Systems", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)), "{:?}", g.options_desc());
    assert!(!g.has_option(g.tgt(giant)), "untapped");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
}

// ---- Sledge-Class Seedship ------------------------------------------------------------

/// Sledge-Class Seedship: "Station. 7+ | Flying" [4/5]; "Whenever this
/// Spacecraft attacks, you may put a creature card from your hand onto the
/// battlefield" (CR 702.184a, 722.2b, 603.5): the creature isn't attacking
/// (CR 508.4: only one put onto the battlefield attacking is).
#[test]
fn sledge_class_seedship_attacks_and_puts_a_creature_from_hand_cr508_4() {
    let mut g = scen()
        .battlefield(0, &["Sledge-Class Seedship"])
        .counters(0, "Sledge-Class Seedship", charge(), 7)
        .hand(0, &["Cross Giant", "Cross Trinket"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Sledge-Class Seedship");
    assert!(is_creature(&g, ship));
    assert_eq!((g.power(ship), g.toughness(ship)), (4, 5));
    assert!(g.has_kw(ship, Kw::Flying));
    attack_with(&mut g, 0, &["Sledge-Class Seedship"]);
    let giant = g.card(0, "Cross Giant", Zone::Hand);
    let trinket = g.card(0, "Cross Trinket", Zone::Hand);
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::YesNo) | Some(DecisionKind::ChooseHandCard)));
    if g.kind() == Some(DecisionKind::YesNo) {
        g.choose(Choice::Yes);
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    }
    assert!(g.has_option(Choice::Card(giant)), "{:?}", g.options_desc());
    assert!(!g.has_option(Choice::Card(trinket)), "a creature card");
    g.choose(Choice::Card(giant));
    g.drive_until(|g| at_priority(g) && g.stack().is_empty());
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "{}", log(&g));
    assert!(!g.obj(giant).attacking, "put onto the battlefield, not attacking");
}

/// The "may": its controller can decline (CR 603.5).
#[test]
fn sledge_class_seedship_may_put_nothing_cr603_5() {
    let mut g = scen()
        .battlefield(0, &["Sledge-Class Seedship"])
        .counters(0, "Sledge-Class Seedship", charge(), 7)
        .hand(0, &["Cross Giant"])
        .at(Step::Main1, 0)
        .start();
    attack_with(&mut g, 0, &["Sledge-Class Seedship"]);
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::YesNo) | Some(DecisionKind::ChooseHandCard)));
    assert!(g.has_option(Choice::Pass), "{:?}", g.options_desc());
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.count_in(0, "Cross Giant", Zone::Hand), 1);
    assert_eq!(g.life(1), 16, "4 combat damage\n{}", log(&g));
}

// ---- Sliver Overlord ------------------------------------------------------------------

/// Sliver Overlord: "{3}: Search your library for a Sliver card, reveal that
/// card, put it into your hand, then shuffle" (CR 701.23a): the search
/// offers Slivers only.
#[test]
fn sliver_overlord_searches_for_a_sliver_card_cr701_23a() {
    let mut g = scen()
        .battlefield(0, &["Sliver Overlord", "Island", "Island", "Island"])
        .library(0, &["Cross Sliver", "Cross Bear", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let o = bf(&g, 0, "Sliver Overlord");
    g.choose(act(o, 0));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::SearchLibrary));
    let sliver = db().id("Cross Sliver").unwrap();
    let bear = db().id("Cross Bear").unwrap();
    assert!(g.has_option(Choice::Def(sliver)), "{:?}", g.options_desc());
    assert!(!g.has_option(Choice::Def(bear)), "not a Sliver");
    g.choose(Choice::Def(sliver));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Cross Sliver", Zone::Hand), 1, "{}", log(&g));
}

/// "{3}: Gain control of target Sliver. (This effect lasts indefinitely.)"
/// (CR 613.1b, 611.2a): the opponent's Sliver stays under P0's control past
/// the end of the turn; a non-Sliver can't be targeted.
#[test]
fn sliver_overlord_gains_control_of_target_sliver_indefinitely_cr613_1b() {
    let mut g = scen()
        .battlefield(0, &["Sliver Overlord", "Island", "Island", "Island"])
        .battlefield(1, &["Cross Sliver", "Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    let o = bf(&g, 0, "Sliver Overlord");
    let (sliver, bear) = (bf(&g, 1, "Cross Sliver"), bf(&g, 1, "Cross Bear"));
    g.choose(act(o, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(sliver)));
    assert!(!g.has_option(g.tgt(bear)), "not a Sliver");
    g.choose(g.tgt(sliver));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(sliver).controller, 0, "{}", log(&g));
    next_main(&mut g, 1);
    assert_eq!(g.obj(sliver).controller, 0, "indefinitely, not until end of turn");
}

// ---- Specimen Freighter ----------------------------------------------------------------

/// Specimen Freighter: "When this Spacecraft enters, return up to two target
/// non-Spacecraft creatures to their owners' hands" (CR 601.2c, 115.1): any
/// controller's; a creature Spacecraft can't be targeted.
#[test]
fn specimen_freighter_returns_up_to_two_non_spacecraft_creatures_cr115_1() {
    let mut g = scen()
        .battlefield(0, &["Island"; 6])
        .battlefield(0, &["Cross Bear"])
        .battlefield(1, &["Cross Giant", "Cross Starship"])
        .hand(0, &["Specimen Freighter"])
        .at(Step::Main1, 0)
        .start();
    let (bear, giant, starship) = (bf(&g, 0, "Cross Bear"), bf(&g, 1, "Cross Giant"), bf(&g, 1, "Cross Starship"));
    cast(&mut g, 0, "Specimen Freighter", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(Choice::Pass), "up to two: none is allowed");
    assert!(!g.has_option(g.tgt(starship)), "a non-Spacecraft creature");
    g.choose(g.tgt(giant));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Hand, "its owner's hand");
    assert_eq!(g.obj(starship).zone, Zone::Battlefield);
}

/// "Station. 9+ | Flying" [4/7]; "Whenever this Spacecraft attacks, defending
/// player mills four cards" (CR 702.184a, 722.2b, 701.17a).
#[test]
fn specimen_freighter_attacks_and_the_defending_player_mills_four_cr701_17a() {
    let g = scen()
        .battlefield(0, &["Specimen Freighter"])
        .counters(0, "Specimen Freighter", charge(), 8)
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Specimen Freighter");
    assert!(!is_creature(&g, ship), "8 counters: not yet");
    let mut g = scen()
        .battlefield(0, &["Specimen Freighter"])
        .counters(0, "Specimen Freighter", charge(), 9)
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Specimen Freighter");
    assert!(is_creature(&g, ship) && g.has_kw(ship, Kw::Flying));
    assert_eq!((g.power(ship), g.toughness(ship)), (4, 7));
    let lib = g.player(1).library.len();
    attack_with(&mut g, 0, &["Specimen Freighter"]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.player(1).library.len(), lib - 4, "{}", log(&g));
    assert_eq!(g.player(1).graveyard.len(), 4);
    assert_eq!(g.life(1), 16);
}

// ---- Starport Security -----------------------------------------------------------------

/// Starport Security: "{3}{W}, {T}: Tap another target creature" (CR
/// 602.2, 701.26a): not itself; three lands aren't enough.
#[test]
fn starport_security_taps_another_target_creature_cr701_26a() {
    let g = scen().battlefield(0, &["Starport Security", "Plains", "Plains", "Plains"]).battlefield(1, &["Cross Bear"]).at(Step::Main1, 0).start();
    let s = bf(&g, 0, "Starport Security");
    assert!(!g.has_option(act(s, 0)), "{{3}}{{W}} with three Plains");
    let mut g = scen()
        .battlefield(0, &["Starport Security", "Plains", "Plains", "Plains", "Plains"])
        .battlefield(1, &["Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    let (s, bear) = (bf(&g, 0, "Starport Security"), bf(&g, 1, "Cross Bear"));
    g.choose(act(s, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(s)), "another creature");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert!(g.obj(bear).tapped && g.obj(s).tapped, "{}", log(&g));
}

/// "This ability costs {2} less to activate if you control a creature with a
/// +1/+1 counter on it" (CR 602.2b, 601.2f): {1}{W} then; an opponent's
/// creature with a counter doesn't count.
#[test]
fn starport_security_costs_2_less_with_a_plus_one_counter_creature_cr602_2b() {
    let g = scen()
        .battlefield(0, &["Starport Security", "Plains", "Plains"])
        .battlefield(1, &["Cross Bear"])
        .counters(1, "Cross Bear", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, "Starport Security");
    assert!(!g.has_option(act(s, 0)), "an opponent's creature doesn't count");
    let mut g = scen()
        .battlefield(0, &["Starport Security", "Plains", "Plains", "Cross Giant"])
        .counters(0, "Cross Giant", Counter::P1P1, 1)
        .battlefield(1, &["Cross Bear"])
        .at(Step::Main1, 0)
        .start();
    let (s, bear) = (bf(&g, 0, "Starport Security"), bf(&g, 1, "Cross Bear"));
    assert!(g.has_option(act(s, 0)), "{{1}}{{W}}: {:?}", g.options_desc());
    g.choose(act(s, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert!(g.obj(bear).tapped, "{}", log(&g));
    assert_eq!(untapped(&g, 0), 0);
}
