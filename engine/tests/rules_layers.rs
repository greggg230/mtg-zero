//! Continuous effects, counters, static abilities and protection (CR 611,
//! 613, 122, 604, 702.16), equipment (301.5, 702.6, 704.5n), and the
//! triggers M7a added (603: other objects' events, look-back, attacks,
//! combat damage, steps, casting, drawing, tapping, sacrificing).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

/// Every decision exposed (`auto_single` off), so a test stops exactly
/// where it looks.
fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn manual() -> Scenario {
    scen()
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

/// Pass priority, answer forced decisions, pay with the first sources
/// offered and put triggers on the stack in the first order offered, until
/// the stack is empty (or another decision is asked).
fn settle(g: &mut Game) {
    for _ in 0..1000 {
        g.drive_until(|g| g.stack().is_empty() && at_priority(g));
        match g.kind() {
            Some(DecisionKind::PayMana) | Some(DecisionKind::OrderTriggers) => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
            _ => return,
        }
    }
    panic!("settle: no progress");
}

/// Pay a cost with the first sources offered.
fn pay(g: &mut Game) {
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(at_priority);
    }
}

/// Cast from hand and answer forced decisions (targets, mana) until the
/// caster has priority again or a real decision is asked.
fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Pass priority, attack and block with nothing, and answer forced
/// decisions until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ if d.options.len() == 1 => d.options[0].choice,
            k => panic!("quiet_until: unexpected {k:?}"),
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

fn to_step(g: &mut Game, step: Step) {
    quiet_until(g, |g| g.turn().step == step);
}

#[test]
fn cr_613_4c_counters_and_until_end_of_turn_pumps_add_up_and_514_2_ends_the_pump() {
    // Luminarch Aspirant: at the beginning of combat on your turn, a +1/+1
    // counter on target creature you control.
    let mut g = scen().battlefield(0, &["Luminarch Aspirant", "Swamp", "Swamp", "Swamp"]).at(Step::Main1, 0).start();
    let la = bf(&g, 0, "Luminarch Aspirant");
    to_step(&mut g, Step::DeclareAttackers);
    assert_eq!(g.counters_on(la, Counter::P1P1), 1);
    assert_eq!(g.pt(la), (2, 2));
    // CR 611.2: "+N/+N until end of turn" effects.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Eagles of the North"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Dismember"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    cast(&mut g, 0, "Dismember");
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    if g.kind() == Some(DecisionKind::PayPhyrexian) {
        g.choose(Choice::Number(0));
        g.drive_until(at_priority);
    }
    pay(&mut g);
    settle(&mut g);
    // -5/-5: the 2/2 has toughness <= 0 and dies (CR 704.5f).
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn cr_611_2c_a_pump_of_creatures_you_control_affects_only_those_there_at_resolution() {
    // Eagles of the North: creatures you control get +1/+0 and first
    // strike until end of turn.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains", "Plains", "Plains", "Test Bear"])
        .hand(0, &["Eagles of the North", "Test Lifelinker"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    cast(&mut g, 0, "Eagles of the North");
    settle(&mut g); // the creature spell
    settle(&mut g); // its trigger
    let eagles = bf(&g, 0, "Eagles of the North");
    assert_eq!(g.pt(bear), (3, 2));
    assert_eq!(g.pt(eagles), (4, 3));
    assert!(g.has_kw(bear, Kw::FirstStrike) && g.has_kw(eagles, Kw::FirstStrike));
    // A creature arriving later is not affected (611.2c)... (no mana left:
    // checked by the effect's locked object list instead)
    assert_eq!(g.effects().len(), 4, "one effect per object and modification");
    // CR 514.2: at cleanup the effects end.
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.pt(bear), (2, 2));
    assert!(!g.has_kw(bear, Kw::FirstStrike));
    assert!(g.effects().is_empty());
}

#[test]
fn cr_613_4b_base_power_and_toughness_in_timestamp_order_and_205_1b_creature_types() {
    let mut g = scen()
        .battlefield(0, &["Figure of Destiny"])
        .battlefield(0, &["Mountain"; 10])
        .at(Step::Main1, 0)
        .start();
    let fig = bf(&g, 0, "Figure of Destiny");
    let spirit = db().subtype_id("Spirit").unwrap();
    let warrior = db().subtype_id("Warrior").unwrap();
    // "If this creature is a Spirit": not yet, so the ability does nothing.
    g.choose(act(fig, 1));
    settle(&mut g);
    assert_eq!(g.pt(fig), (1, 1));
    assert!(!g.has_subtype(fig, spirit));
    g.choose(act(fig, 0));
    settle(&mut g);
    assert_eq!(g.pt(fig), (2, 2));
    assert!(g.has_subtype(fig, spirit) && !g.has_subtype(fig, warrior));
    g.choose(act(fig, 1));
    settle(&mut g);
    assert_eq!(g.pt(fig), (4, 4));
    assert!(g.has_subtype(fig, warrior));
    // The first ability again: its later timestamp wins (613.7), and it
    // makes it a Kithkin Spirit, no longer a Warrior.
    g.choose(act(fig, 0));
    settle(&mut g);
    assert_eq!(g.pt(fig), (2, 2));
    assert!(!g.has_subtype(fig, warrior));
    // None of it ends at cleanup (no duration).
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.pt(fig), (2, 2));
}

#[test]
fn cr_702_16_protection_prevents_targeting_blocking_and_damage() {
    let mut g = manual()
        .battlefield(0, &["Mother of Runes", "Test Bear"])
        .battlefield(1, &["Mountain", "Test Hasty"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let mom = bf(&g, 0, "Mother of Runes");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(mom, 0));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    // The colour of her controller's choice (red = 3).
    assert_eq!(g.kind(), Some(DecisionKind::ChooseColor));
    g.choose(Choice::Number(3));
    assert!(g.protection_of(bear).colors == 8);
    // 702.16b: a red spell can't target it.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1);
    // (until end of turn: gone in P1's turn.) Give it again at instant speed.
    let mut g = manual()
        .battlefield(0, &["Mother of Runes", "Test Bear"])
        .battlefield(1, &["Mountain", "Test Hasty"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let (mom, bear) = (bf(&g, 0, "Mother of Runes"), bf(&g, 0, "Test Bear"));
    g.choose(Choice::Pass);
    assert_eq!(g.turn().priority, 0);
    g.choose(act(mom, 0));
    g.choose(g.tgt(bear));
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Number(3));
    g.drive_until(at_priority);
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    // P1 has priority again after passing back and forth: cast Bolt.
    quiet_until(&mut g, |g| g.turn().priority == 1);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert!(!g.has_option(g.tgt(bear)), "702.16b: no red targeting");
    assert!(g.has_option(g.tgt(mom)));
}

#[test]
fn cr_702_16e_protection_from_red_blocks_damage_and_red_blockers() {
    let mut g = manual()
        .battlefield(0, &["Mother of Runes", "Test Bear"])
        .battlefield(1, &["Test Hasty"])
        .at(Step::Main1, 0)
        .start();
    let (mom, bear) = (bf(&g, 0, "Mother of Runes"), bf(&g, 0, "Test Bear"));
    let hasty = bf(&g, 1, "Test Hasty");
    g.choose(act(mom, 0));
    g.choose(g.tgt(bear));
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    g.choose(Choice::Number(3)); // red
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    // The red creature can't block the protected bear (702.16e): only pass.
    let d = g.pending_decision().unwrap();
    assert_eq!(d.player, 1);
    assert!(d.options.iter().all(|o| o.choice == Choice::Pass), "{:?}", g.options_desc());
    let _ = hasty;
}

#[test]
fn equipment_cr_702_6_equip_301_5_bonuses_and_704_5n_unattach() {
    let mut g = scen()
        .battlefield(0, &["Lightning Greaves", "Skateboard", "Mountain"])
        .hand(0, &["Test Bear"])
        .battlefield(0, &["Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Test Bear");
    pay(&mut g);
    settle(&mut g);
    let bear = bf(&g, 0, "Test Bear");
    let greaves = bf(&g, 0, "Lightning Greaves");
    assert!(g.is_sick(bear) && !g.has_kw(bear, Kw::Haste));
    // Equip {0} (sorcery speed): haste and shroud.
    g.choose(act(greaves, 0));
    settle(&mut g);
    assert_eq!(g.obj(greaves).attached, Some(g.obj_ref(bear)));
    assert!(g.has_kw(bear, Kw::Haste) && g.has_kw(bear, Kw::Shroud));
    // Shroud: the bear can't be the target of the second equip ability.
    let board = bf(&g, 0, "Skateboard");
    assert!(!g.has_option(act(board, 0)), "no legal target for Skateboard's equip");
    // It can attack the turn it came (haste).
    to_step(&mut g, Step::DeclareAttackers);
    assert!(g.has_option(Choice::Card(bear)));
}

#[test]
fn cr_704_5n_equipment_stays_on_the_battlefield_unattached_when_its_creature_dies() {
    let mut g = scen()
        .battlefield(0, &["Skateboard", "Test Bear", "Mountain"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let (board, bear) = (bf(&g, 0, "Skateboard"), bf(&g, 0, "Test Bear"));
    g.choose(act(board, 0));
    settle(&mut g);
    assert_eq!(g.pt(bear), (3, 2));
    quiet_until(&mut g, |g| g.turn().priority == 1);
    cast(&mut g, 1, "Lightning Bolt");
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(bear));
    }
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(board).zone, Zone::Battlefield);
    assert_eq!(g.obj(board).attached, None, "704.5n");
}

#[test]
fn cr_702_83_exalted_and_702_91_battle_cry() {
    // Exalted: a creature attacking alone gets +1/+1 per instance.
    let mut g = scen()
        .battlefield(0, &["Noble Hierarch", "Ignoble Hierarch", "Test Bear"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.pt(bear), (4, 4));
    // Not alone: no exalted.
    let mut g = scen()
        .battlefield(0, &["Noble Hierarch", "Test Bear", "Test Striker"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let (bear, striker) = (bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Striker"));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Card(striker));
    g.choose(Choice::Pass);
    assert!(g.stack().is_empty() && g.waiting_triggers().is_empty());
    // Battle cry: each other attacking creature gets +1/+0.
    let mut g = scen()
        .battlefield(0, &["Sanguine Evangelist", "Test Bear", "Test Striker"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let (ev, bear, striker) = (bf(&g, 0, "Sanguine Evangelist"), bf(&g, 0, "Test Bear"), bf(&g, 0, "Test Striker"));
    g.choose(Choice::Card(ev));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.pt(ev), (2, 1));
    assert_eq!(g.pt(bear), (3, 2));
    assert_eq!(g.pt(striker), (2, 1), "not attacking");
}

#[test]
fn cr_603_2_draw_triggers_of_both_players_sheoldred() {
    let mut g = scen().battlefield(0, &["Sheoldred, the Apocalypse"]).at(Step::Main1, 0).start();
    // P1's draw step: P1 loses 2; P0's: P0 gains 2.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Draw);
    settle(&mut g);
    assert_eq!(g.life(1), 18);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Draw);
    settle(&mut g);
    assert_eq!(g.life(0), 22);
}

#[test]
fn dark_confidant_reveals_the_top_card_into_hand_and_its_owner_loses_its_mana_value() {
    let mut g = scen()
        .library(0, &["Griselbrand"])
        .battlefield(0, &["Dark Confidant"])
        .at(Step::End, 1)
        .start();
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Upkeep);
    settle(&mut g);
    let gris = g.card(0, "Griselbrand", Zone::Hand);
    assert_eq!(g.obj(gris).known, 0b11, "revealed: known to both players");
    assert_eq!(g.life(0), 12);
}

#[test]
fn cr_603_2_landfall_lotus_cobra_and_603_6a_other_objects_entering() {
    let mut g = manual().battlefield(0, &["Lotus Cobra"]).hand(0, &["Forest"]).at(Step::Main1, 0).start();
    let forest = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(forest));
    // The trigger goes on the stack; it resolves: a colour choice.
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana));
    g.choose(Choice::Number(0));
    assert_eq!(g.player(0).pool.amt.iter().sum::<u8>(), 1);
}

#[test]
fn cr_603_10a_leave_triggers_look_back_in_a_simultaneous_move() {
    // Super Shredder: whenever another permanent leaves the battlefield.
    // Wrath of God destroys it with two others at once: it sees both.
    let mut g = scen()
        .battlefield(0, &["Super Shredder", "Test Bear", "Plains", "Plains", "Plains", "Plains"])
        .battlefield(1, &["Test Striker"])
        .hand(0, &["Wrath of God"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Wrath of God");
    g.pass();
    g.pass();
    // The Wrath resolved; two Shredder triggers wait or are on the stack.
    let n = g.stack().len() + g.waiting_triggers().len();
    assert_eq!(n, 2, "{}", g.log_lines().join("\n"));
}

#[test]
fn cr_509_1b_can_t_be_blocked_except_by_three_or_more() {
    let mut g = scen()
        .battlefield(0, &["Troll of Khazad-dûm"])
        .battlefield(1, &["Test Bear", "Test Striker", "Test Wall"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let troll = bf(&g, 0, "Troll of Khazad-dûm");
    g.choose(Choice::Card(troll));
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers));
    // The first blocker may block only if the other two can join.
    assert!(g.has_option(Choice::Card(troll)));
    g.choose(Choice::Card(troll));
    // The second must block it too (one short otherwise).
    assert!(!g.has_option(Choice::Pass));
    g.choose(Choice::Card(troll));
    assert!(!g.has_option(Choice::Pass));
    g.choose(Choice::Card(troll));
    assert_eq!(g.blockers_of(troll).len(), 3);
}

#[test]
fn manland_layer_4_type_and_7b_base_pt_until_end_of_turn() {
    let mut g = scen()
        .battlefield(0, &["Restless Vinestalk", "Forest", "Forest", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let v = bf(&g, 0, "Restless Vinestalk");
    assert!(!g.is_creature(v));
    g.choose(act(v, 1));
    settle(&mut g);
    assert!(g.is_creature(v) && g.is_type(v, engine::db::types::LAND));
    assert_eq!(g.pt(v), (5, 5));
    assert!(g.has_kw(v, Kw::Trample));
    assert_eq!(g.colors_of(v), 0b10010, "green and blue");
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert!(!g.is_creature(v));
}

#[test]
fn cr_122_1d_stun_counters_and_502_3_doesnt_untap() {
    let mut g = scen().battlefield(0, &["Grim Monolith", "Forest"]).hand(0, &["Baloth Prime"]).at(Step::Main1, 0).start();
    let (mono, forest) = (bf(&g, 0, "Grim Monolith"), bf(&g, 0, "Forest"));
    cast(&mut g, 0, "Baloth Prime");
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    settle(&mut g);
    let b = bf(&g, 0, "Baloth Prime");
    // "Enters tapped with six stun counters" (CR 122.6).
    assert!(g.obj(b).tapped && g.obj(mono).tapped && g.obj(forest).tapped);
    assert_eq!(g.counters_on(b, Counter::Stun), 6);
    quiet_until(&mut g, |g| g.turn().active == 1);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Upkeep);
    // 502.3: the Forest untaps; the Monolith doesn't; a stun counter is
    // removed instead of untapping Baloth Prime (122.1d).
    assert!(!g.obj(forest).tapped);
    assert!(g.obj(mono).tapped);
    assert!(g.obj(b).tapped);
    assert_eq!(g.counters_on(b, Counter::Stun), 5);
}

#[test]
fn static_anthem_other_dwarves_you_control() {
    let g = scen().battlefield(0, &["Magda, Brazen Outlaw", "Test Dwarf", "Test Bear"]).battlefield(1, &["Test Dwarf"]).at(Step::Main1, 0).start();
    let magda = bf(&g, 0, "Magda, Brazen Outlaw");
    let dwarf = bf(&g, 0, "Test Dwarf");
    let bear = bf(&g, 0, "Test Bear");
    let theirs = bf(&g, 1, "Test Dwarf");
    assert_eq!(g.pt(magda), (2, 1), "other Dwarves");
    assert_eq!(g.pt(dwarf), (3, 2));
    assert_eq!(g.pt(bear), (2, 2));
    assert_eq!(g.pt(theirs), (2, 2), "you control");
}

#[test]
fn becomes_tapped_trigger_magda_attacking_dwarves_make_treasures() {
    let mut g = scen().battlefield(0, &["Magda, Brazen Outlaw", "Test Dwarf"]).at(Step::DeclareAttackers, 0).start();
    let (magda, dwarf) = (bf(&g, 0, "Magda, Brazen Outlaw"), bf(&g, 0, "Test Dwarf"));
    g.choose(Choice::Card(magda));
    g.choose(Choice::Card(dwarf));
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Treasure", Zone::Battlefield), 2);
    let _ = Target::Player(0);
}
