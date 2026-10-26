//! M8d part 3 (ENGINE.md 39): the cube's trigger rulings left after the
//! triage was narrowed: reflexive triggers and their targets (CR 603.12),
//! last known information (608.2h), sources gone or returned (603.7,
//! 400.7), "whenever you attack" without the source attacking (508.3a),
//! power checked as a creature enters (603.6a, 603.10), magecraft, miracle
//! and draws (702.94, 121), intervening "if" clauses (603.4), the order of
//! simultaneous triggers (603.3b), triggers of copies and of spells that
//! leave the stack. `ruling: <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ab(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn pay(g: &mut Game) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::PayPool)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// Declare `attackers` (at P1) and pass.
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &a in attackers {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(1));
        }
    }
    g.choose(Choice::Pass);
}

fn first_non_pass(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass)
}

/// P`p` casts `name` from hand at `target` (an object) and everything
/// resolves.
fn cast_at(g: &mut Game, p: u8, name: &str, target: CardIdx) {
    let c = g.card(p, name, Zone::Hand);
    let t = g.tgt(target);
    g.choose(Choice::Cast(c));
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
}

/// The option, at an `OrderTriggers` decision, putting next the waiting
/// trigger of the decider whose source is named `name`.
fn first_trigger_from(g: &Game, name: &str) -> Option<Choice> {
    let d = g.pending_decision().unwrap();
    let mine: Vec<_> = g.waiting_triggers().iter().filter(|t| t.controller == d.player).collect();
    let k = mine.iter().position(|t| db().def(t.source_def).name == name)?;
    let c = Choice::Trigger(k as u8);
    g.has_option(c).then_some(c)
}

// ---- reflexive triggers (CR 603.12) ------------------------------------------------------------

#[test]
fn a_reflexive_trigger_chooses_its_target_when_put_on_the_stack_and_can_be_answered_cr_603_12() {
    // ruling: 2aa2b091 d9f993eb 47eed49e 9e9b6606
    // Guide of Souls: P0 pays {E}{E}{E} as the attack trigger resolves, then
    // the reflexive "when you do" triggers, its target chosen then, and P1
    // gets priority with it on the stack. Ajani, Nacatl Avenger's +2 under
    // Elspeth makes two Cat Warriors: its reflexive damage triggers once
    // for each.
    let mut g = scen()
        .battlefield(0, &["Guide of Souls", "Test Bear", "Mountain"])
        .hand(0, &["Galvanic Discharge"])
        .battlefield(1, &["Test Wall"])
        .at(Step::Main1, 0)
        .start();
    let wall = bf(&g, 1, "Test Wall");
    let gd = g.card(0, "Galvanic Discharge", Zone::Hand);
    g.choose(Choice::Cast(gd));
    let t = g.tgt(wall);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)),
        _ => None,
    });
    let bear = bf(&g, 0, "Test Bear");
    attack(&mut g, &[bear]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    assert_eq!(g.stack().len(), 1, "the attack trigger resolving");
    g.choose(Choice::Yes);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "the reflexive trigger's target, as it goes on the stack\n{}", log(&g));
    g.choose(g.tgt(bear));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.stack().len(), 1, "P1 may respond to the reflexive trigger\n{}", log(&g));
    // Ajani, Nacatl Avenger's 0 ("create a 2/1 Cat Warrior; when you do, if
    // you control a red permanent other than Ajani, he deals damage equal
    // to the number of creatures you control to any target") doubled.
    let ajani = "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger";
    let mut g = scen()
        .battlefield(0, &["Elspeth, Storm Slayer", ajani, "Mountain", "Test Hasty"])
        .transformed(0, ajani)
        .at(Step::Main1, 0)
        .start();
    let av = bf(&g, 0, "Ajani, Nacatl Avenger");
    let a = (0..3u8).map(|i| ab(av, i)).find(|&a| {
        let mut h = g.clone();
        h.choose(a);
        resolve_with(&mut h, |_| None);
        !tokens_named(&h, "Cat Warrior").is_empty()
    });
    g.choose(a.expect("Ajani's Cat Warrior ability"));
    let targets = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            targets.set(targets.get() + 1);
            Some(player(1))
        }
        _ => None,
    });
    assert_eq!(tokens_named(&g, "Cat Warrior").len(), 2, "{}", log(&g));
    assert_eq!(targets.get(), 2, "one reflexive trigger per Cat Warrior, each targeting as it's put on the stack\n{}", log(&g));
}

#[test]
fn a_reflexive_trigger_whose_target_is_gone_draws_nothing_cr_603_12_608_2b() {
    // ruling: a391f90a
    // Minsc & Boo makes Boo (a Hamster) at P0's upkeep; its -2 sacrifices
    // Boo, and the reflexive trigger targets P1's Bear; P0 Bolts the Bear in
    // response: the trigger doesn't resolve, so no card is drawn though Boo
    // was a Hamster.
    let mut g = scen()
        .battlefield(0, &["Minsc & Boo, Timeless Heroes", "Mountain"])
        .counters(0, "Minsc & Boo, Timeless Heroes", Counter::Loyalty, 5)
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Upkeep, 0)
        .start();
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty(), |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    let boo = tokens_named(&g, "Boo");
    assert_eq!(boo.len(), 1, "{}", log(&g));
    let mb = bf(&g, 0, "Minsc & Boo, Timeless Heroes");
    let bear = bf(&g, 1, "Test Bear");
    let hand = g.player(0).hand.len();
    let slot = (0..3u8).rev().map(|i| ab(mb, i)).find(|&a| g.has_option(a)).expect("-2");
    g.choose(slot);
    quiet_until_with(&mut g, |g| at_priority(g) && g.turn().priority == 0 && g.obj(boo[0]).zone != Zone::Battlefield && !g.stack().is_empty(), |g| match g.kind() {
        Some(DecisionKind::Sacrifice) | Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(boo[0])),
        Some(DecisionKind::Target) if g.has_option(g.tgt(bear)) => Some(g.tgt(bear)),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    cast_at(&mut g, 0, "Lightning Bolt", bear);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), hand - 1, "no draw: the Bolt left the hand, nothing came
{}", log(&g));
}

// ---- last known information (CR 608.2h, 113.7a) ------------------------------------------------

#[test]
fn abilities_whose_source_left_use_its_last_known_values_cr_608_2h() {
    // ruling: ef3a8866 b3653e3c 509660c9 ed19c884 b9423c74 8eb07966
    // Jacked Rabbit (a counter: a 2/3) attacks; P1 Bolts it with the
    // trigger waiting: two Rabbits. Ouroboroid's beginning-of-combat
    // trigger (power 1 + 2 counters = 3), Ouroboroid exiled: three counters
    // on each other creature. Caustic Bronco, saddled, attacks and is
    // Bolted with its trigger waiting: the opponent loses the life.
    // Malcolm (four chorus counters) Bolted with its trigger waiting: no
    // fifth counter, but still the loot and the free cast (four counters
    // when it left). Norman Osborn Bolted with his
    // connive waiting: he still connives (draw, discard), no counter.
    let bolt_it = |g: &mut Game, c: CardIdx| {
        g.drive_until(|g| at_priority(g) && g.turn().priority == 1 && !g.stack().is_empty());
        cast_at(g, 1, "Lightning Bolt", c);
    };
    let p1 = |sc: engine::testkit::Scenario| sc.battlefield(1, &["Mountain"]).hand(1, &["Lightning Bolt"]);
    // Jacked Rabbit.
    let mut g = p1(scen().battlefield(0, &["Jacked Rabbit"]).counters(0, "Jacked Rabbit", Counter::P1P1, 1).at(Step::DeclareAttackers, 0)).start();
    let jr = bf(&g, 0, "Jacked Rabbit");
    attack(&mut g, &[jr]);
    bolt_it(&mut g, jr);
    assert_eq!(g.obj(jr).zone, Zone::Graveyard);
    assert_eq!(tokens_named(&g, "Rabbit").len(), 2, "power 2 when it left\n{}", log(&g));
    // Ouroboroid.
    let mut g = scen()
        .battlefield(0, &["Ouroboroid", "Test Wall"])
        .counters(0, "Ouroboroid", Counter::P1P1, 2)
        .battlefield(1, &["Plains"])
        .hand(1, &["Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    let ou = bf(&g, 0, "Ouroboroid");
    let wall = bf(&g, 0, "Test Wall");
    g.drive_until(|g| g.turn().step == Step::BeginCombat && !g.stack().is_empty() && at_priority(g) && g.turn().priority == 0);
    g.pass();
    cast_at(&mut g, 1, "Swords to Plowshares", ou);
    assert_eq!(g.obj(ou).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.counters_on(wall, Counter::P1P1), 3, "X from its last known power\n{}", log(&g));
    // Caustic Bronco, saddled.
    let mut g = p1(scen().battlefield(0, &["Caustic Bronco", "Test Giant"]).library(0, &["Fiery Confluence", "Island"]).at(Step::Main1, 0)).start();
    let br = bf(&g, 0, "Caustic Bronco");
    let gi = bf(&g, 0, "Test Giant");
    g.choose(ab(br, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(gi)).then_some(Choice::Card(gi)));
    attack(&mut g, &[br]);
    bolt_it(&mut g, br);
    assert_eq!(g.obj(br).zone, Zone::Graveyard);
    assert_eq!((g.life(0), g.life(1)), (20, 16), "saddled when it left: the opponent loses 4\n{}", log(&g));
    // Malcolm.
    let chorus = Counter::Named(db().subtype_id("counter:chorus").unwrap());
    let mut g = p1(scen()
        .battlefield(0, &["Malcolm, Alluring Scoundrel"])
        .counters(0, "Malcolm, Alluring Scoundrel", chorus, 4)
        .hand(0, &["Chain Lightning"])
        .library(0, &["Island", "Island"])
        .at(Step::DeclareAttackers, 0))
    .start();
    let m = bf(&g, 0, "Malcolm, Alluring Scoundrel");
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    attack(&mut g, &[m]);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1 && !g.stack().is_empty());
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    let t = g.tgt(m);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::Discard) | Some(DecisionKind::ChooseDiscard) if g.has_option(Choice::Card(cl)) => Some(Choice::Card(cl)),
        Some(DecisionKind::YesNo) if g.pending_decision().unwrap().player == 0 => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(m).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20 - 2 - 3, "looted and cast for free (four counters when it left)\n{}", log(&g));
    // Norman Osborn.
    let mut g = p1(scen().battlefield(0, &["Norman Osborn // Green Goblin"]).hand(0, &["Test Bear"]).library(0, &["Island", "Island"]).at(Step::DeclareAttackers, 0)).start();
    let no = bf(&g, 0, "Norman Osborn // Green Goblin");
    let hand = g.player(0).hand.len();
    attack(&mut g, &[no]);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1 && !g.stack().is_empty());
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    let t = g.tgt(no);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        _ => None,
    });
    assert_eq!(g.obj(no).zone, Zone::Graveyard);
    assert!(g.player(0).graveyard.len() >= 2, "he connived: drew and discarded\n{}", log(&g));
    assert_eq!(g.player(0).hand.len(), hand, "one drawn, one discarded");
}

// ---- sources gone or returned (CR 603.7, 400.7) ------------------------------------------------

#[test]
fn delayed_returns_happen_without_their_source_and_a_returned_permanent_misses_this_end_step_cr_603_7() {
    // ruling: 2237cc62 d29b2c9d
    // Phelia attacks and exiles P1's Test Wall (it returns at the next end
    // step); Phelia is Bolted: the Wall returns all the same. Flickerwisp
    // exiles P0's own Ocelot Pride (life gained this turn): it returns at
    // the beginning of the end step, too late for its own end-step trigger.
    let mut g = scen()
        .battlefield(0, &["Phelia, Exuberant Shepherd"])
        .battlefield(1, &["Test Wall", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let ph = bf(&g, 0, "Phelia, Exuberant Shepherd");
    let wall = bf(&g, 1, "Test Wall");
    attack(&mut g, &[ph]);
    let t = g.tgt(wall);
    quiet_until_with(&mut g, |g| g.obj(wall).zone == Zone::Exile && at_priority(g), |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 1);
    cast_at(&mut g, 1, "Lightning Bolt", ph);
    assert_eq!(g.obj(ph).zone, Zone::Graveyard);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.card(1, "Test Wall", Zone::Battlefield).0 > 0 || g.count_in(1, "Test Wall", Zone::Battlefield) == 1, true, "{}", log(&g));
    assert_eq!(g.count_in(1, "Test Wall", Zone::Battlefield), 1, "returned without Phelia\n{}", log(&g));
    // Flickerwisp and Ocelot Pride.
    let mut g = scen()
        .battlefield(0, &["Ocelot Pride", "Test Lifelinker", "Plains", "Plains", "Plains"])
        .hand(0, &["Flickerwisp"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let ll = bf(&g, 0, "Test Lifelinker");
    let oc = bf(&g, 0, "Ocelot Pride");
    attack(&mut g, &[ll]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.life(0), 22);
    let fw = g.card(0, "Flickerwisp", Zone::Hand);
    g.choose(Choice::Cast(fw));
    let t = g.tgt(oc);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(oc).zone, Zone::Exile);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.count_in(0, "Ocelot Pride", Zone::Battlefield), 1, "{}", log(&g));
    assert!(tokens_named(&g, "Cat").is_empty(), "it missed this end step's beginning\n{}", log(&g));
}

#[test]
fn whenever_you_attack_triggers_without_its_source_attacking_cr_508_3a() {
    // ruling: ab682fd6 cdd6186e
    // Adeline and Gut stay home; a Bear attacks: Adeline makes a Human,
    // Gut may sacrifice an artifact for a Skeleton.
    let mut g = scen()
        .battlefield(0, &["Adeline, Resplendent Cathar", "Gut, True Soul Zealot", "Test Bear", "Mox Pearl"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    let pearl = bf(&g, 0, "Mox Pearl");
    attack(&mut g, &[bear]);
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main2, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::Sacrifice) | Some(DecisionKind::ChoosePermanent) if g.has_option(Choice::Card(pearl)) => Some(Choice::Card(pearl)),
        _ => None,
    });
    assert_eq!(tokens_named(&g, "Human").len(), 1, "{}", log(&g));
    assert_eq!(tokens_named(&g, "Skeleton").len(), 1, "{}", log(&g));
}

// ---- power checked as a creature enters (CR 603.6a, 603.10) ------------------------------------

#[test]
fn an_entering_creatures_power_counts_its_counters_and_later_changes_dont_matter_cr_603_6a() {
    // ruling: 3efcc01c 95367116 0c836eb3
    // Enduring Innocence ("other creatures with power 2 or less enter: draw,
    // once each turn"): Walking Ballista with X = 3 enters as a 3/3, no draw;
    // with X = 2, a draw. Vaultborn Tyrant ("another creature with power 4
    // or greater enters: gain 3, draw"): Walking Ballista X = 4 triggers it;
    // the Ballista removed in response, the trigger still resolves.
    for (x, draws) in [(3u8, 0usize), (2, 1)] {
        let mut g = scen().battlefield(0, &["Enduring Innocence"]).battlefield(0, &["Island"; 6]).hand(0, &["Walking Ballista"]).at(Step::Main1, 0).start();
        let wb = g.card(0, "Walking Ballista", Zone::Hand);
        let hand = g.player(0).hand.len();
        g.choose(Choice::Cast(wb));
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
        g.choose(Choice::Number(x));
        pay(&mut g);
        resolve(&mut g);
        assert_eq!(g.player(0).hand.len(), hand - 1 + draws, "X = {x}\n{}", log(&g));
    }
    let mut g = scen()
        .battlefield(0, &["Vaultborn Tyrant"])
        .battlefield(0, &["Island"; 8])
        .hand(0, &["Walking Ballista"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &["Sink into Stupor // Soporific Springs"])
        .at(Step::Main1, 0)
        .start();
    let wb = g.card(0, "Walking Ballista", Zone::Hand);
    g.choose(Choice::Cast(wb));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(4));
    pay(&mut g);
    g.drive_until(|g| g.obj(wb).zone == Zone::Battlefield && at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.stack().len(), 1, "the Tyrant's trigger\n{}", log(&g));
    let life = g.life(0);
    cast_at(&mut g, 1, "Sink into Stupor // Soporific Springs", wb);
    assert_eq!(g.obj(wb).zone, Zone::Hand);
    assert_eq!(g.life(0), life + 3, "the trigger resolved anyway\n{}", log(&g));
}

#[test]
fn sword_of_the_meek_sees_static_1_1s_and_is_returned_whatever_changes_after_cr_603_6a() {
    // ruling: d80bd9c5 d3c2bdb3 2bb24717
    // Cori-Steel Cutter's Monk (a 1/1) enters: Sword of the Meek (in the
    // graveyard as it enters) triggers; the Monk's prowess pumps it before
    // the trigger resolves: the Sword still returns and attaches.
    let mut g = scen()
        .battlefield(0, &["Cori-Steel Cutter", "Mountain", "Mountain", "Mountain"])
        .graveyard(0, &["Sword of the Meek"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let sword = g.card(0, "Sword of the Meek", Zone::Graveyard);
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    cast_targets(&mut g, b, &[player(1)]);
    // The second spell: the Monk (no attachment), and the Sword's trigger
    // waits on the stack.
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    quiet_until_with(&mut g, |g| !tokens_named(g, "Monk").is_empty() && at_priority(g) && g.turn().priority == 0, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        Some(DecisionKind::Priority) => Some(Choice::Pass),
        _ => None,
    });
    let monk = tokens_named(&g, "Monk")[0];
    assert_eq!(g.pt(monk), (1, 1));
    // The third: prowess makes the Monk a 2/2 before the Sword's trigger
    // resolves; it returns and attaches all the same.
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::YesNo) if g.stack().len() <= 2 && g.obj(sword).zone == Zone::Graveyard => Some(Choice::Yes),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(sword).zone, Zone::Battlefield, "returned for the 1/1 Monk
{}", log(&g));
    assert_eq!(g.obj(sword).attached.map(|r| r.idx), Some(monk));
}
