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
use engine::db::{AbilitySlot, Counter};
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

// ---- spells that leave the stack; their cast triggers (CR 603.2, 702.157, 702.175) -------------

#[test]
fn cast_triggers_resolve_without_their_spell_and_countered_creatures_make_no_tokens_cr_603_2() {
    // ruling: b3cee2a5 8de0da15 591acf16 fb8555e1 c961b628 3d567b94
    // Thundertrap Trainer with offspring and Sicarian Infiltrator with
    // squad, countered: no tokens. Sowing Mycospawn's cast trigger (search
    // a land) resolves though Mycospawn is countered. Pinnacle Emissary's
    // "whenever you cast an artifact spell" Drone, the spell countered. A
    // Trainer that resolved and was Bolted before its offspring trigger:
    // the token still comes, and its own enter ability triggers.
    let counter = |g: &mut Game, spell: CardIdx| {
        g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
        let cs = g.card(1, "Counterspell", Zone::Hand);
        g.choose(Choice::Cast(cs));
        let t = g.tgt(spell);
        resolve_with(g, |g| match g.kind() {
            Some(DecisionKind::Target) if g.has_option(t) => Some(t),
            Some(DecisionKind::SearchLibrary) => first_non_pass(g),
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            _ => None,
        });
    };
    let p1 = |sc: engine::testkit::Scenario| sc.battlefield(1, &["Island", "Island"]).hand(1, &["Counterspell"]);
    for (name, n) in [("Thundertrap Trainer", 6usize), ("Sicarian Infiltrator", 8)] {
        let mut g = p1(scen().battlefield(0, &vec!["Island"; n]).hand(0, &[name]).at(Step::Main1, 0)).start();
        let c = g.card(0, name, Zone::Hand);
        g.choose(Choice::Cast(c));
        g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) || at_priority(g));
        if matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) {
            let max = g.pending_decision().unwrap().options.iter().map(|o| o.choice).filter(|c| matches!(c, Choice::Number(_))).last().unwrap();
            g.choose(max);
        }
        pay(&mut g);
        counter(&mut g, c);
        assert_eq!(g.obj(c).zone, Zone::Graveyard);
        assert!(tokens_named(&g, name).is_empty(), "{name}: countered, no tokens\n{}", log(&g));
    }
    // Sowing Mycospawn.
    let mut g = p1(scen().battlefield(0, &["Forest"; 4]).hand(0, &["Sowing Mycospawn"]).library(0, &["Island", "Taiga", "Island"]).at(Step::Main1, 0)).start();
    let sm = g.card(0, "Sowing Mycospawn", Zone::Hand);
    let lands = g.lands_of(0).count();
    g.choose(Choice::Cast(sm));
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) || at_priority(g));
    if matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) {
        g.choose(Choice::Number(0));
    }
    pay(&mut g);
    counter(&mut g, sm);
    assert_eq!(g.obj(sm).zone, Zone::Graveyard);
    assert_eq!(g.lands_of(0).count(), lands + 1, "the search resolved\n{}", log(&g));
    // Pinnacle Emissary.
    let mut g = p1(scen().battlefield(0, &["Pinnacle Emissary", "Island", "Island"]).hand(0, &["Sol Ring"]).at(Step::Main1, 0)).start();
    let ring = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(ring));
    pay(&mut g);
    counter(&mut g, ring);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert_eq!(tokens_named(&g, "Drone").len(), 1, "{}", log(&g));
    // Thundertrap Trainer Bolted before its offspring trigger resolves.
    let mut g = scen()
        .battlefield(0, &["Island"; 6])
        .hand(0, &["Thundertrap Trainer"])
        .library(0, &["Island", "Island", "Island", "Island", "Island"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let tt = g.card(0, "Thundertrap Trainer", Zone::Hand);
    g.choose(Choice::Cast(tt));
    g.drive_until(|g| matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) || at_priority(g));
    if matches!(g.kind(), Some(DecisionKind::ChooseCost) | Some(DecisionKind::ChooseMode)) {
        g.choose(Choice::Number(1));
    }
    pay(&mut g);
    quiet_until_with(&mut g, |g| at_priority(g) && g.turn().priority == 1 && g.obj(tt).zone == Zone::Battlefield, |g| (g.kind() == Some(DecisionKind::OrderTriggers)).then(|| first_trigger_from(g, "Thundertrap Trainer")).flatten());
    cast_at(&mut g, 1, "Lightning Bolt", tt);
    let toks = tokens_named(&g, "Thundertrap Trainer");
    assert_eq!(toks.len(), 1, "the token though the Trainer is gone\n{}", log(&g));
    assert!(log(&g).matches("trigger of Thundertrap Trainer goes on the stack").count() >= 3, "the token's own enter ability too\n{}", log(&g));
}

// ---- Flash, Cecil, Reanimate: life and triggers (CR 603.3, 704.5a) -----------------------------

#[test]
fn flash_declined_the_creature_entered_and_its_enter_and_dies_triggers_go_together_cr_603_3() {
    // ruling: 0fb23eca 70ba8133
    // Flash puts Archon of Cruelty onto the battlefield; P0 doesn't pay:
    // sacrificed at once. Its enter trigger happened (P1 sacrifices,
    // discards, loses 3; P0 draws, gains 3).
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Flash", "Archon of Cruelty"])
        .battlefield(1, &["Test Bear"])
        .hand(1, &["Island"])
        .at(Step::Main1, 0)
        .start();
    let fl = g.card(0, "Flash", Zone::Hand);
    let ar = g.card(0, "Archon of Cruelty", Zone::Hand);
    g.choose(Choice::Cast(fl));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => Some(Choice::Card(ar)),
        Some(DecisionKind::YesNo) if g.pending_decision().unwrap().player == 0 && g.obj(ar).zone == Zone::Battlefield && g.stack().len() <= 1 => Some(Choice::Pass),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.obj(ar).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(1), 17, "its enter trigger resolved\n{}", log(&g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
}

#[test]
fn reanimates_life_loss_comes_first_and_cecil_dying_as_it_deals_damage_still_triggers_cr_603_10_704_5a() {
    // ruling: 0a7655f0 7f9205e7
    // Reanimate on Archon of Cruelty at 8 life: P0 loses 8 first and the
    // game is over before the Archon's trigger resolves. Cecil, Dark Knight
    // blocked by a Test Giant (deathtouch) dies as it deals damage: its
    // trigger still makes P0 lose that much life, and it doesn't transform.
    let mut g = scen().battlefield(0, &["Swamp"]).graveyard(0, &["Archon of Cruelty"]).hand(0, &["Reanimate"]).life(0, 8).at(Step::Main1, 0).start();
    let ar = g.card(0, "Archon of Cruelty", Zone::Graveyard);
    let re = g.card(0, "Reanimate", Zone::Hand);
    let t = g.tgt(ar);
    g.choose(Choice::Cast(re));
    quiet_until_with(&mut g, |g| g.is_over(), |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert!(g.is_over() && g.player(0).lost, "{}", log(&g));
    assert_eq!(g.life(1), 20, "the Archon's trigger never resolved");
    let cecil = "Cecil, Dark Knight // Cecil, Redeemed Paladin";
    let mut g = scen().battlefield(0, &[cecil]).battlefield(1, &["Test Giant"]).life(0, 12).at(Step::DeclareAttackers, 0).start();
    let c = bf(&g, 0, cecil);
    let giant = bf(&g, 1, "Test Giant");
    attack(&mut g, &[c]);
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main2, |g| match g.kind() {
        Some(DecisionKind::DeclareBlockers) => {
            if g.has_option(Choice::Card(giant)) {
                Some(Choice::Card(giant))
            } else if g.has_option(Choice::Card(c)) {
                Some(Choice::Card(c))
            } else {
                Some(Choice::Pass)
            }
        }
        _ => None,
    });
    assert_eq!(g.obj(c).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(0), 10, "Cecil's trigger: P0 loses the 2 it dealt\n{}", log(&g));
    assert!(!g.name(c).starts_with("Cecil, Redeemed"), "not transformed (it left)");
}

// ---- magecraft and miracle (CR 702.150, 702.94) ------------------------------------------------

#[test]
fn every_magecraft_triggers_and_a_prepare_copy_triggers_only_when_cast_cr_702_150() {
    // ruling: 73ddf41c f41a9b99 3cf2e3a4 e120a54f
    // Sedgemoor Witch and Witherbloom Apprentice: a Lightning Bolt triggers
    // both (a Pest, 1 drain). Emeritus of Abundance entering prepared (its
    // Regrowth copy made in exile) triggers neither; casting the copy does.
    let em = "Emeritus of Abundance // Regrowth";
    let mut g = scen()
        .battlefield(0, &["Sedgemoor Witch", "Witherbloom Apprentice", "Mountain"])
        .battlefield(0, &["Forest"; 5])
        .graveyard(0, &["Island"])
        .hand(0, &["Lightning Bolt", em])
        .at(Step::Main1, 0)
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    cast_targets(&mut g, b, &[player(1)]);
    assert_eq!(tokens_named(&g, "Pest").len(), 1);
    assert_eq!(g.life(1), 16, "3 and the Apprentice's 1");
    let e = g.card(0, em, Zone::Hand);
    g.choose(Choice::Cast(e));
    resolve(&mut g);
    assert!(g.is_prepared(e));
    assert_eq!(tokens_named(&g, "Pest").len(), 1, "the copy made in exile triggers nothing");
    assert_eq!(g.life(1), 16);
    let copy = g.exile_zone().iter().copied().find(|&c| g.is_copy_obj(c) && g.name(c) == "Regrowth").unwrap();
    let isl = g.card(0, "Island", Zone::Graveyard);
    g.choose(Choice::Cast(copy));
    let t = g.tgt(isl);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(tokens_named(&g, "Pest").len(), 2, "casting the copy\n{}", log(&g));
    assert_eq!(g.life(1), 15);
}

#[test]
fn miracle_on_the_first_draw_any_turn_only_as_its_trigger_resolves_the_card_drawn_either_way_cr_702_94() {
    // ruling: 0f9f54eb bf463705 33d4b423 0cb05459 f353546d 8ff49c69 1790c518 e61f99f9
    // On P1's turn, P0's first draw (Brainstorm) is Metamorphosis Fanatic:
    // miracle offered. Declined, it stays in P0's hand (drawn: Sheoldred's
    // trigger sees it) and can't be cast for its miracle cost later. Only
    // the first of Brainstorm's three draws may be revealed. Demonic Tutor
    // puts it into the hand without a draw: no miracle, no Bowmasters
    // trigger, not revealed.
    let mf = "Metamorphosis Fanatic";
    let mut g = scen()
        .battlefield(0, &["Island", "Swamp", "Swamp"])
        .hand(0, &["Brainstorm"])
        .library(0, &[mf, "Island", "Island", "Island"])
        .battlefield(1, &["Sheoldred, the Apocalypse"])
        .at(Step::Main1, 1)
        .start();
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    let f = g.card(0, mf, Zone::Library);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    g.choose(Choice::Cast(bs));
    let asked = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) if g.pending_decision().unwrap().player == 0 => {
            asked.set(asked.get() + 1);
            Some(Choice::Pass)
        }
        Some(DecisionKind::PutOnTop) | Some(DecisionKind::ChooseHandCard) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Card(f) && c != Choice::Pass),
        _ => None,
    });
    assert_eq!(asked.get(), 1, "the reveal offered for the first draw only\n{}", log(&g));
    assert_eq!(g.obj(f).zone, Zone::Hand, "kept in hand");
    assert_eq!(g.life(0), 14, "drawn: Sheoldred's 2 for each of the three\n{}", log(&g));
    // Demonic Tutor: no miracle, no Bowmasters, not revealed.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Demonic Tutor"])
        .library(0, &["Island", mf, "Island"])
        .battlefield(1, &["Orcish Bowmasters"])
        .at(Step::Main1, 0)
        .start();
    let dt = g.card(0, "Demonic Tutor", Zone::Hand);
    let f = g.card(0, mf, Zone::Library);
    g.choose(Choice::Cast(dt));
    let asked = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => Some(Choice::Def(id(mf))),
        Some(DecisionKind::YesNo) => {
            asked.set(true);
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(g.obj(f).zone, Zone::Hand);
    assert!(!asked.get(), "no miracle: not drawn\n{}", log(&g));
    assert_eq!(g.life(0), 20, "no Bowmasters trigger");
    assert!(!g.knows(1, f), "not revealed");
}

// ---- draws counted over the turn (CR 121.2) ----------------------------------------------------

#[test]
fn draw_limits_and_draw_counters_see_draws_before_their_source_entered_cr_121_2() {
    // ruling: 814381be 618eec3a f872e80b
    // P1 drew once this turn (Ancestral Recall is three draws...): P1 draws
    // one card (the scenario's Test May Draw), then P0 flashes in Faerie
    // Mastermind: P1's next draw is the second this turn, Mastermind
    // triggers. Leovold or Narset entering after P1 drew a card: P1 can't
    // draw more this turn.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Faerie Mastermind"])
        .battlefield(1, &["Island", "Island", "Island", "Island"])
        .hand(1, &["Test May Draw", "Brainstorm"])
        .at(Step::Main1, 1)
        .start();
    let md = g.card(1, "Test May Draw", Zone::Hand);
    g.choose(Choice::Cast(md));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    // P0 flashes in the Mastermind (P1 passes with an empty stack? P1
    // casts Brainstorm; P0 responds).
    let bs = g.card(1, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let fm = g.card(0, "Faerie Mastermind", Zone::Hand);
    g.choose(Choice::Cast(fm));
    let hand0 = g.player(0).hand.len();
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutOnTop) | Some(DecisionKind::ChooseHandCard) => first_non_pass(g),
        _ => None,
    });
    assert_eq!(g.player(0).hand.len(), hand0 + 1, "one card drawn for P1's second draw this turn\n{}", log(&g));
    for limiter in ["Leovold, Emissary of Trest", "Narset, Parter of Veils"] {

        // (The limiters can't be cast on P1's turn: P1 draws on P0's turn,
        // by P0's Ancestral Recall, then the limiter enters.)
        let mut g = scen()
            .battlefield(0, &["Island", "Island", "Swamp", "Forest", "Plains", "Island"])
            .hand(0, &[limiter, "Ancestral Recall"])
            .battlefield(1, &["Island"])
            .hand(1, &["Ancestral Recall"])
            .at(Step::Main1, 0)
            .start();
        let ar = g.card(0, "Ancestral Recall", Zone::Hand);
        cast_targets(&mut g, ar, &[player(1)]);
        let after = g.player(1).hand.len();
        let lim = g.card(0, limiter, Zone::Hand);
        g.choose(Choice::Cast(lim));
        resolve(&mut g);
        // P1 Ancestral Recalls itself (instant) on P0's turn: no draw.
        g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
        g.pass();
        let ar1 = g.card(1, "Ancestral Recall", Zone::Hand);
        if g.has_option(Choice::Cast(ar1)) {
            cast_targets(&mut g, ar1, &[player(1)]);
            assert_eq!(g.player(1).hand.len(), after - 1, "{limiter}: the earlier draws count\n{}", log(&g));
        }
    }
}

// ---- simultaneous triggers ordered by their controller (CR 603.3b) -----------------------------

#[test]
fn simultaneous_draws_let_sheoldreds_controller_order_and_landfall_triggers_are_ordered_cr_603_3b() {
    // ruling: 628c3597 6ea84d35
    // Timetwister makes both players draw seven at once: Sheoldred's
    // controller orders the triggers. A land entering with Lotus Cobra and
    // Bristly Bill: P0 orders the two landfall triggers.
    let mut g = scen()
        .battlefield(0, &["Sheoldred, the Apocalypse", "Island", "Island", "Island"])
        .hand(0, &["Timetwister"])
        .library(0, &["Island"; 10])
        .library(1, &["Forest"; 10])
        .at(Step::Main1, 0)
        .start();
    let tt = g.card(0, "Timetwister", Zone::Hand);
    g.choose(Choice::Cast(tt));
    let ordered = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::OrderTriggers) {
            assert_eq!(g.pending_decision().unwrap().player, 0, "Sheoldred's controller orders");
            ordered.set(true);
        }
        None
    });
    assert!(ordered.get(), "{}", log(&g));
    assert_eq!((g.life(0), g.life(1)), (34, 6), "7 x 2 each way");
    let mut g = scen().battlefield(0, &["Lotus Cobra", "Bristly Bill, Spine Sower"]).hand(0, &["Forest"]).at(Step::Main1, 0).start();
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    g.drive_until(|g| g.kind() == Some(DecisionKind::OrderTriggers) || (at_priority(g) && g.stack().is_empty()));
    assert_eq!(g.kind(), Some(DecisionKind::OrderTriggers), "{}", log(&g));
}

// ---- Palace Jailer and the monarch (CR 724) ----------------------------------------------------

#[test]
fn palace_jailers_exile_lasts_until_an_opponent_becomes_monarch_even_if_p0_isnt_monarch_now_cr_724_610_3() {
    // ruling: 867620c4
    // P1 is the monarch. P0's Palace Jailer: P0 orders its exile trigger to
    // resolve before its monarch trigger, so as the Bear is exiled P1 (an
    // opponent) is the monarch: the Bear stays exiled (it returns when an
    // opponent *becomes* the monarch), also once P0 is the monarch; P1
    // taking the monarchy back by combat damage returns it.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains"])
        .hand(0, &["Palace Jailer"])
        .battlefield(1, &["Test Bear", "Test Flyer"])
        .monarch(1)
        .at(Step::Main1, 0)
        .start();
    let pj = g.card(0, "Palace Jailer", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Cast(pj));
    pay(&mut g);
    let t = g.tgt(bear);
    let first = std::cell::Cell::new(true);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        // (The first put on the stack resolves last: the monarch trigger.)
        Some(DecisionKind::OrderTriggers) if first.get() => {
            first.set(false);
            let d = g.pending_decision().unwrap();
            let mine: Vec<_> = g.waiting_triggers().iter().filter(|w| w.controller == d.player).collect();
            // (Palace Jailer's first ability, "you become the monarch".)
            let k = mine.iter().position(|w| w.slot == 0)?;
            Some(Choice::Trigger(k as u8))
        }
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile, "{}", log(&g));
    // P1's Flyer hits P0: P1 becomes the monarch: the Bear returns.
    quiet_until_with(&mut g, |g| g.turn().active == 1 && g.kind() == Some(DecisionKind::DeclareAttackers), |_| None);
    let fl = bf(&g, 1, "Test Flyer");
    g.choose(Choice::Card(fl));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(0));
    }
    g.choose(Choice::Pass);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1, "{}", log(&g));
}

// ---- Archon of Cruelty's order (CR 608.2c) -----------------------------------------------------

#[test]
fn archon_of_cruelty_the_sacrifice_comes_before_the_discard_and_the_life_cr_608_2c() {
    // ruling: 89a61120
    // P1's only creature is sacrificed first; then P1 discards and loses 3:
    // Test Dies Draw's "when this dies, draw a card" waits until the
    // ability has finished, so P1 can't discard the card it would draw.
    let mut g = scen()
        .battlefield(0, &["Swamp"])
        .graveyard(0, &["Archon of Cruelty"])
        .hand(0, &["Reanimate"])
        .battlefield(1, &["Test Dies Draw"])
        .hand(1, &["Island"])
        .library(1, &["Lightning Bolt", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let ar = g.card(0, "Archon of Cruelty", Zone::Graveyard);
    let re = g.card(0, "Reanimate", Zone::Hand);
    let t = g.tgt(ar);
    g.choose(Choice::Cast(re));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.count_in(1, "Island", Zone::Graveyard), 1, "the Island discarded (the only card then)\n{}", log(&g));
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Hand), 1, "the dies-draw came after");
}

// ---- revolt and dash (CR 702.109) -------------------------------------------------------------

#[test]
fn revolt_counts_any_permanent_leaving_and_dash_returns_only_a_creature_still_there_cr_702_109a() {
    // ruling: 7995d7c4 d96d165d
    // Polluted Delta sacrificed (any permanent leaving counts): Fatal Push
    // has revolt and destroys the 4-mana Test Trampler.
    // Ragavan dashed and Bolted: it stays in the graveyard at the end step.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Polluted Delta"])
        .library(0, &["Island", "Swamp"])
        .hand(0, &["Fatal Push"])
        .battlefield(1, &["Test Trampler"])
        .at(Step::Main1, 0)
        .start();
    let tr = bf(&g, 1, "Test Trampler");
    let pd = bf(&g, 0, "Polluted Delta");
    let pa = (0..3u8).map(|i| ab(pd, i)).find(|&a| g.has_option(a)).expect("the fetch");
    g.choose(pa);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| first_non_pass(g)).flatten());
    assert_eq!(g.obj(pd).zone, Zone::Graveyard, "{}", log(&g));
    let fp = g.card(0, "Fatal Push", Zone::Hand);
    let t = g.tgt(tr);
    g.choose(Choice::Cast(fp));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(tr).zone, Zone::Graveyard, "revolt: mana value 4 or less\n{}", log(&g));
    // Ragavan dashed, Bolted: not returned.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Ragavan, Nimble Pilferer"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let rag = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    g.choose(Choice::Cast(rag));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(1)));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0 && g.stack().is_empty());
    g.pass();
    cast_at(&mut g, 1, "Lightning Bolt", rag);
    quiet_until(&mut g, |g| g.turn().active == 1);
    assert_eq!(g.obj(rag).zone, Zone::Graveyard, "{}", log(&g));
}

// ---- intervening "if" and conditions checked twice (CR 603.4) ----------------------------------

#[test]
fn conditions_on_trigger_and_resolution_robber_tersa_caves_magda_cr_603_4() {
    // ruling: bcb139f0 08172cc2 903365a8 dc0e9b36
    // Robber of the Rich attacks with P1 holding more cards: it triggers;
    // P1 plays down to fewer before it resolves (Brainstorm? an instant
    // P1 casts) - the comparison is made again: nothing exiled. Its
    // permission outlasts it: Robber Bolted, a Rogue attacking on a later
    // turn still lets P0 cast the card. Tersa Lightshatter's attack trigger
    // with seven cards in P0's graveyard, which P1's Ghost Vacuum shrinks to
    // six in response: nothing happens. Magda: a Dwarf already tapped isn't
    // "becoming tapped" again (Test Tapper's tap at it): no Treasure.
    // Robber: P1 (three cards) has more than P0 (two) as it attacks; P1
    // casts a Bolt in response: two and two, nothing is exiled.
    let mut g = scen()
        .battlefield(0, &["Robber of the Rich"])
        .hand(0, &["Island", "Island"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt", "Island", "Island"])
        .library(1, &["Forest"; 4])
        .at(Step::DeclareAttackers, 0)
        .start();
    let rob = bf(&g, 0, "Robber of the Rich");
    attack(&mut g, &[rob]);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1 && !g.stack().is_empty());
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(0)));
    assert!(g.exile_zone().is_empty(), "two cards against two on resolution\n{}", log(&g));
    // Its permission outlasts it: exiled card (a Bolt), Robber Bolted by
    // P1, then on P0's next turn the Rogue Thieving Skydiver attacks: P0
    // may cast the exiled Bolt.
    let mut g = scen()
        .battlefield(0, &["Robber of the Rich", "Thieving Skydiver", "Mountain"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt", "Island"])
        .library(1, &["Lightning Bolt", "Forest", "Forest", "Forest"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let rob = bf(&g, 0, "Robber of the Rich");
    attack(&mut g, &[rob]);
    resolve(&mut g);
    let ex = g.exile_zone().to_vec();
    assert_eq!(ex.len(), 1, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    cast_at(&mut g, 1, "Lightning Bolt", rob);
    assert_eq!(g.obj(rob).zone, Zone::Graveyard);
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let sky = bf(&g, 0, "Thieving Skydiver");
    g.choose(Choice::Card(sky));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    assert!(g.has_option(Choice::Cast(ex[0])), "a Rogue attacked this turn: {}", opts(&g));
    // Tersa.
    let mut g = scen()
        .battlefield(0, &["Tersa Lightshatter"])
        .graveyard(0, &["Island"; 7])
        .battlefield(1, &["Ghost Vacuum"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let te = bf(&g, 0, "Tersa Lightshatter");
    attack(&mut g, &[te]);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1 && !g.stack().is_empty());
    let gv = bf(&g, 1, "Ghost Vacuum");
    g.choose(ab(gv, 0));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| first_non_pass(g)).flatten());
    assert_eq!(g.player(0).graveyard.len(), 6, "six cards: the trigger does nothing\n{}", log(&g));
    assert_eq!(g.exile_zone().len(), 1, "only the Vacuum's card");
    // Magda.
    let mut g = scen()
        .battlefield(0, &["Magda, Brazen Outlaw", "Test Dwarf", "Test Hasty Tapper"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let dw = bf(&g, 0, "Test Dwarf");
    let tap = bf(&g, 0, "Test Hasty Tapper");
    attack(&mut g, &[dw]);
    resolve(&mut g);
    let t1 = tokens_named(&g, "Treasure").len();
    assert_eq!(t1, 1, "attacking tapped the Dwarf\n{}", log(&g));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    if g.has_option(ab(tap, 0)) {
        g.choose(ab(tap, 0));
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(dw))).then(|| g.tgt(dw)));
        assert_eq!(tokens_named(&g, "Treasure").len(), 1, "already tapped: no new Treasure\n{}", log(&g));
    }
}

// ---- monstrosity, Phlage, Nadu, Vinestalk, Toxic Deluge ----------------------------------------

#[test]
fn deathclaw_gone_before_monstrosity_resolves_doesnt_trigger_and_phlage_not_escaped_is_sacrificed_cr_701_37_702_138() {
    // ruling: 956681d4 0d9e6436 8e644e8c e08d64dc
    // Alpha Deathclaw's monstrosity, the Deathclaw Swords-ed in response:
    // no "becomes monstrous" trigger (P1's Sol Ring survives). Phlage put
    // onto the battlefield by Reanimate: its enter trigger (3 damage, 3
    // life) and its sacrifice (not escaped). Toxic Deluge: a creature that
    // enters afterwards that turn isn't affected.
    let mut g = scen()
        .battlefield(0, &["Alpha Deathclaw"])
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Forest", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Sol Ring", "Plains"])
        .hand(1, &["Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    let ad = bf(&g, 0, "Alpha Deathclaw");
    g.choose(ab(ad, 0));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    cast_at(&mut g, 1, "Swords to Plowshares", ad);
    assert_eq!(g.obj(ad).zone, Zone::Exile);
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Battlefield), 1, "no monstrous trigger\n{}", log(&g));
    // Phlage reanimated.
    let mut g = scen().battlefield(0, &["Swamp"]).graveyard(0, &["Phlage, Titan of Fire's Fury"]).hand(0, &["Reanimate"]).at(Step::Main1, 0).start();
    let ph = g.card(0, "Phlage, Titan of Fire's Fury", Zone::Graveyard);
    let re = g.card(0, "Reanimate", Zone::Hand);
    let t = g.tgt(ph);
    g.choose(Choice::Cast(re));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.obj(ph).zone, Zone::Graveyard, "sacrificed (not escaped)\n{}", log(&g));
    assert_eq!(g.life(1), 17, "its enter trigger");
    assert_eq!(g.life(0), 20 - 3 + 3, "Reanimate's 3, Phlage's 3 back");
    // Toxic Deluge then a creature.
    let mut g = scen().battlefield(0, &["Swamp", "Swamp", "Swamp", "Forest", "Forest"]).hand(0, &["Toxic Deluge", "Test Bear"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let td = g.card(0, "Toxic Deluge", Zone::Hand);
    g.choose(Choice::Cast(td));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(2)));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    let b = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve(&mut g);
    assert_eq!(g.pt(b), (2, 2), "entered later: unaffected\n{}", log(&g));
}

#[test]
fn nadus_granted_triggers_resolve_without_nadu_and_reset_for_a_new_nadu_cr_603_2_400_7() {
    // ruling: be65848a
    // Nadu: the Bear targeted by Mother of Runes triggers Nadu's granted
    // ability; Nadu exiled while it waits: it still resolves (a land to
    // the battlefield or a card to hand).
    let mut g = scen()
        .battlefield(0, &["Nadu, Winged Wisdom", "Mother of Runes", "Test Bear"])
        .library(0, &["Forest", "Island", "Forest", "Island"])
        .battlefield(1, &["Plains"])
        .hand(1, &["Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    let nadu = bf(&g, 0, "Nadu, Winged Wisdom");
    let bear = bf(&g, 0, "Test Bear");
    let lib = g.player(0).library.len();
    mother_at(&mut g, bear, 3);
    // P0 holds; the Nadu trigger waits; P1 Bolts Nadu (P1 has priority
    // after P0 passes).
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1 && !g.stack().is_empty());
    let b = g.card(1, "Swords to Plowshares", Zone::Hand);
    g.choose(Choice::Cast(b));
    let t = g.tgt(nadu);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(3)),
        _ => None,
    });
    assert_eq!(g.obj(nadu).zone, Zone::Exile);
    assert!(g.player(0).library.len() < lib, "the granted trigger resolved without Nadu\n{}", log(&g));
}

/// Mother of Runes gives `c` protection from colour `color`, answering
/// everything; the Mother's ability waits on the stack afterwards? No: it
/// resolves; triggers it caused wait.
fn mother_at(g: &mut Game, c: CardIdx, color: u8) {
    let m = bf(g, 0, "Mother of Runes");
    g.choose(ab(m, 0));
    let t = g.tgt(c);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(t);
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::ChooseColor));
    if g.kind() == Some(DecisionKind::ChooseColor) {
        g.choose(Choice::Number(color));
    }
}

#[test]
fn a_land_animated_by_another_effect_still_has_its_attack_trigger_cr_603_2() {
    // ruling: 4a5ce741
    // Restless Vinestalk made a 1/1 by Life attacks: its "whenever this
    // attacks, up to one other target creature has base 3/3" triggers.
    let mut g = scen()
        .battlefield(0, &["Restless Vinestalk", "Test Bear"])
        .hand(0, &["Life // Death"])
        .pool(0, [0, 0, 0, 0, 1, 0], 0)
        .at(Step::Main1, 0)
        .start();
    let rv = bf(&g, 0, "Restless Vinestalk");
    let bear = bf(&g, 0, "Test Bear");
    let ld = g.card(0, "Life // Death", Zone::Hand);
    g.choose(Choice::Cast(ld));
    resolve(&mut g);
    assert!(g.is_creature(rv));
    attack(&mut g, &[rv]);
    let triggered = std::cell::Cell::new(false);
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main2, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            triggered.set(true);
            g.has_option(g.tgt(bear)).then(|| g.tgt(bear))
        }
        Some(DecisionKind::YesNo) => {
            triggered.set(true);
            Some(Choice::Yes)
        }
        _ => None,
    });
    assert!(triggered.get(), "{}", log(&g));
}

// ---- combat-related triggers -------------------------------------------------------------------

#[test]
fn coveted_jewel_resolves_after_blocks_before_damage_with_one_attacker_unblocked_cr_509_1h() {
    // ruling: 1ec0d4c1 8d31adf9
    // P1 attacks P0 (Coveted Jewel's controller) with two Bears; P0 blocks
    // one with its Wall: the Jewel's trigger, then the unblocked Bear's
    // damage after (P1 draws three and takes the Jewel before damage).
    let mut g = scen()
        .battlefield(0, &["Coveted Jewel", "Test Wall"])
        .battlefield(1, &["Test Bear", "Test Bear"])
        .at(Step::DeclareAttackers, 1)
        .start();
    let jewel = bf(&g, 0, "Coveted Jewel");
    let wall = bf(&g, 0, "Test Wall");
    let bears: Vec<CardIdx> = g.permanents_of(1).filter(|&c| g.name(c) == "Test Bear").collect();
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for &b in &bears {
        g.choose(Choice::Card(b));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(0));
        }
    }
    g.choose(Choice::Pass);
    let blocked = std::cell::Cell::new(false);
    quiet_until_with(&mut g, |g| g.obj(jewel).controller == 1, |g| match g.kind() {
        Some(DecisionKind::DeclareBlockers) if !blocked.get() && g.has_option(Choice::Card(wall)) => Some(Choice::Card(wall)),
        Some(DecisionKind::DeclareBlockers) if !blocked.get() && g.has_option(Choice::Card(bears[0])) => {
            blocked.set(true);
            Some(Choice::Card(bears[0]))
        }
        _ => None,
    });
    assert_eq!(g.obj(jewel).controller, 1);
    assert_eq!(g.life(0), 20, "before combat damage\n{}", log(&g));
    assert!(matches!(g.turn().step, Step::DeclareBlockers));
}

// ---- more sources and their events --------------------------------------------------------------

#[test]
fn laelia_an_opponents_exile_counts_and_rockets_destroyed_draw_and_a_crewed_copter_doesnt_enter_cr_603_2_603_6a() {
    // ruling: a898c85e 7e54db0f 526a6c5c
    // P1's Ghost Vacuum exiles a card from P0's graveyard: Laelia gets a
    // counter. Wizard's Rockets destroyed by P1's Abrade: P0 draws. Smuggler's
    // Copter crewed: Guide of Souls ("another creature you control enters")
    // doesn't trigger.
    let mut g = scen()
        .battlefield(0, &["Laelia, the Blade Reforged", "Wizard's Rockets"])
        .graveyard(0, &["Island"])
        .battlefield(1, &["Ghost Vacuum", "Mountain", "Mountain"])
        .hand(1, &["Abrade"])
        .at(Step::Main1, 1)
        .start();
    let la = bf(&g, 0, "Laelia, the Blade Reforged");
    let isl = g.card(0, "Island", Zone::Graveyard);
    let gv = bf(&g, 1, "Ghost Vacuum");
    g.choose(ab(gv, 0));
    let t = g.tgt(isl);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.counters_on(la, Counter::P1P1), 1, "{}", log(&g));
    let wr = bf(&g, 0, "Wizard's Rockets");
    let hand = g.player(0).hand.len();
    let a = g.card(1, "Abrade", Zone::Hand);
    g.choose(Choice::Cast(a));
    let t = g.tgt(wr);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        _ => None,
    });
    assert_eq!(g.obj(wr).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), hand + 1, "destroyed, not sacrificed: still a draw\n{}", log(&g));
    let mut g = scen().battlefield(0, &["Guide of Souls", "Smuggler's Copter", "Test Bear"]).at(Step::Main1, 0).start();
    let copter = bf(&g, 0, "Smuggler's Copter");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(ab(copter, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(bear)).then_some(Choice::Card(bear)));
    assert!(g.is_creature(copter));
    assert_eq!((g.life(0), g.player(0).energy), (20, 0), "no enter trigger\n{}", log(&g));
}

#[test]
fn an_earthbended_land_bounced_isnt_returned_and_teferis_untaps_are_chosen_at_the_end_step_cr_603_7() {
    // ruling: b4e0984f 956752d0
    // Badgermole Cub earthbends a Forest; P1's Jace, the Mind Sculptor
    // returns it to P0's hand: the "when it dies or is exiled" return
    // doesn't apply.
    // Teferi, Hero of Dominaria's +1: the two lands to untap are chosen at
    // the next end step. His emblem: the target is chosen after the draw.
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest"])
        .hand(0, &["Badgermole Cub"])
        .battlefield(1, &["Jace, the Mind Sculptor"])
        .at(Step::Main1, 0)
        .start();
    let cub = g.card(0, "Badgermole Cub", Zone::Hand);
    let f = bf(&g, 0, "Forest");
    g.choose(Choice::Cast(cub));
    let t = g.tgt(f);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert!(g.is_creature(f));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    // Jace's -1: "return target creature to its owner's hand".
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    let a = (0..4u8).map(|i| ab(jace, i)).find(|&a| {
        let mut h = g.clone();
        h.choose(a);
        h.drive_until(|h| h.kind() == Some(DecisionKind::Target) || at_priority(h));
        h.kind() == Some(DecisionKind::Target) && h.has_option(t)
    });
    g.choose(a.expect("Jace's -1"));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1);
    assert_eq!(g.obj(f).zone, Zone::Hand, "{}", log(&g));
    // Teferi's +1.
    let mut g = scen()
        .battlefield(0, &["Teferi, Hero of Dominaria", "Island", "Island", "Island"])
        .hand(0, &["Ancestral Recall", "Brainstorm"])
        .library(0, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    let tf = bf(&g, 0, "Teferi, Hero of Dominaria");
    g.choose(ab(tf, 0));
    resolve(&mut g);
    let ar = g.card(0, "Ancestral Recall", Zone::Hand);
    cast_targets(&mut g, ar, &[player(0)]);
    let b = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve(&mut g);
    let tapped = g.lands_of(0).filter(|&l| g.obj(l).tapped).count();
    assert_eq!(tapped, 2);
    let asked = std::cell::Cell::new(false);
    quiet_until_with(&mut g, |g| g.turn().active == 1, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) | Some(DecisionKind::Target) if g.turn().step == Step::End => {
            asked.set(true);
            first_non_pass(g)
        }
        _ => None,
    });
    assert!(asked.get(), "chosen at the end step\n{}", log(&g));
}

#[test]
fn ursine_triggers_at_each_combat_with_no_main_phase_between_and_persist_returns_apnap_cr_506_1_603_3b() {
    // ruling: c298a763 de5ecf09
    // Fear of Missing Out (delirium) attacking: an extra combat right after
    // the first (no main phase between); Ursine Monstrosity's beginning-of-
    // combat trigger again. Woodfall Primus (P0) and P1's Metamorph copy of
    // it die to P0's Toxic Deluge: P0's persist trigger goes on the stack
    // first, P1's resolves first.
    let mut g = scen()
        .battlefield(0, &["Fear of Missing Out", "Ursine Monstrosity"])
        .graveyard(0, &["Mox Pearl", "Island", "Test Giant", "Ancestral Recall"])
        .library(0, &["Island"; 6])
        .at(Step::Main1, 0)
        .start();
    let fomo = bf(&g, 0, "Fear of Missing Out");
    let mut steps = Vec::new();
    let mut attacks = 0;
    for _ in 0..400 {
        if g.turn().step == Step::Main2 || g.turn().active == 1 {
            break;
        }
        if steps.last() != Some(&g.turn().step) {
            steps.push(g.turn().step);
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::DeclareAttackers if attacks == 0 => {
                attacks += 1;
                Choice::Card(fomo)
            }
            DecisionKind::DeclareAttackers if g.has_option(Choice::Card(fomo)) && attacks == 1 && d.options.len() > 1 => {
                attacks += 1;
                Choice::Card(fomo)
            }
            DecisionKind::AttackTarget => player(1),
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            DecisionKind::Target => first_non_pass(&g).unwrap_or(d.options[0].choice),
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    let begins = steps.iter().filter(|&&s| s == Step::BeginCombat).count();
    assert_eq!(begins, 2, "two combats: {steps:?}\n{}", log(&g));
    let i = steps.iter().position(|&s| s == Step::EndCombat).unwrap();
    assert_eq!(steps[i + 1], Step::BeginCombat, "no main phase between: {steps:?}");
    assert_eq!(log(&g).matches("trigger of Ursine Monstrosity goes on the stack").count(), 2, "{}", log(&g));
}

#[test]
fn persist_triggers_go_on_the_stack_apnap_so_the_nonactive_players_creature_returns_first_cr_603_3b() {
    // ruling: 54494365
    // P0's Woodfall Primus and P1's Metamorph copying it die to P0's
    // Toxic Deluge: P0's persist trigger is put on the stack first, P1's on
    // top: P1's Primus returns first.
    let mut g = scen()
        .battlefield(0, &["Woodfall Primus", "Swamp", "Swamp", "Swamp"])
        .battlefield(1, &["Island"; 4])
        .hand(1, &["Phyrexian Metamorph"])
        .hand(0, &["Toxic Deluge"])
        .at(Step::Main1, 1)
        .start();
    let wp = bf(&g, 0, "Woodfall Primus");
    let m = g.card(1, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(m));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => Some(Choice::Card(wp)),
        Some(DecisionKind::Target) => first_non_pass(g),
        _ => None,
    });
    assert_eq!(g.name(m), "Woodfall Primus", "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    let td = g.card(0, "Toxic Deluge", Zone::Hand);
    g.choose(Choice::Cast(td));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(6)),
        Some(DecisionKind::Target) => first_non_pass(g),
        _ => None,
    });
    let lines = g.log_lines();
    let back = |name: &str, who: &str| lines.iter().position(|l| l.contains(&format!("{name} Graveyard -> Battlefield")) && (who.is_empty() || l.contains(who)));
    let p1_back = back("Phyrexian Metamorph", "");
    let p0_back = back("Woodfall Primus", "");
    assert!(p1_back.is_some() && p0_back.is_some(), "both persisted\n{}", log(&g));
    assert!(p1_back < p0_back, "the nonactive player's first\n{}", log(&g));
}

#[test]
fn nadu_entering_after_a_spell_targeted_misses_it_and_a_new_nadus_ability_counts_anew_cr_603_2_400_7() {
    // ruling: 1b428e03
    // P1 Bolts P0's Test Wall; in response P0 Flashes in Nadu: the Wall
    // didn't have Nadu's ability as it became the target: no trigger.
    let mut g = scen()
        .battlefield(0, &["Test Wall", "Forest", "Island", "Forest", "Island"])
        .hand(0, &["Flash", "Nadu, Winged Wisdom"])
        .library(0, &["Forest", "Island", "Forest"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let wall = bf(&g, 0, "Test Wall");
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(wall));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let fl = g.card(0, "Flash", Zone::Hand);
    let nadu = g.card(0, "Nadu, Winged Wisdom", Zone::Hand);
    let lib = g.player(0).library.len();
    g.choose(Choice::Cast(fl));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => Some(Choice::Card(nadu)),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(nadu).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.player(0).library.len(), lib, "no Nadu trigger for the Bolt\n{}", log(&g));
}

#[test]
fn teferis_emblem_targets_after_the_draw_cr_603_3d() {
    // ruling: 18387350
    // Teferi, Hero of Dominaria's -8 emblem; P0's Brainstorm draws: each
    // draw's trigger chooses its target as it is put on the stack, after
    // Brainstorm (and the drawing) finished.
    let mut g = scen()
        .battlefield(0, &["Teferi, Hero of Dominaria", "Island"])
        .counters(0, "Teferi, Hero of Dominaria", Counter::Loyalty, 8)
        .hand(0, &["Brainstorm"])
        .library(0, &["Island"; 5])
        .battlefield(1, &["Sol Ring", "Test Bear", "Mox Pearl"])
        .at(Step::Main1, 0)
        .start();
    let tf = bf(&g, 0, "Teferi, Hero of Dominaria");
    let a = (0..3u8).rev().map(|i| ab(tf, i)).find(|&a| g.has_option(a)).expect("-8");
    g.choose(a);
    resolve(&mut g);
    let b = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(b));
    let mut targets_after_draws = true;
    for _ in 0..100 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        if d.kind == DecisionKind::Target && g.obj(b).zone == Zone::Stack {
            targets_after_draws = false;
        }
        let c = match d.kind {
            DecisionKind::Priority => Choice::Pass,
            _ => first_non_pass(&g).unwrap_or(d.options[0].choice),
        };
        g.choose(c);
    }
    assert!(targets_after_draws, "no target chosen while Brainstorm resolved");
    assert_eq!(log(&g).matches("trigger of Teferi, Hero of Dominaria emblem goes on the stack").count(), 3, "three draws\n{}", log(&g));
}

#[test]
fn caves_of_chaos_checks_a_completed_dungeon_as_it_exiles_cr_603_4() {
    // ruling: e25495c7 4f2c74cd
    // No dungeon completed as Caves of Chaos Adventurer's trigger exiles a
    // Lightning Bolt: P0 may play it this turn, paying; completing
    // Undercity later that turn (Test Initiative from the last room) doesn't
    // make it free, so with no red mana it can't be cast. With a completed
    // dungeon as it exiles: free, castable without mana.
    let mut g = scen()
        .battlefield(0, &["Caves of Chaos Adventurer", "Island"])
        .hand(0, &["Test Initiative"])
        .library(0, &["Lightning Bolt", "Island", "Island", "Island"])
        .dungeon(0, Some(7), 0)
        .at(Step::DeclareAttackers, 0)
        .start();
    let cc = bf(&g, 0, "Caves of Chaos Adventurer");
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    attack(&mut g, &[cc]);
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert_eq!(g.obj(bolt).zone, Zone::Exile, "{}", log(&g));
    let ti = g.card(0, "Test Initiative", Zone::Hand);
    g.choose(Choice::Cast(ti));
    resolve_with(&mut g, |g| first_non_pass(g));
    assert!(g.dungeon(0).1 >= 1, "completed now\n{}", log(&g));
    g.drive_until(|g| at_priority(g) && g.stack().is_empty());
    assert!(!g.has_option(Choice::Cast(bolt)), "not free: completed after the exile: {}", opts(&g));
    let mut g = scen()
        .battlefield(0, &["Caves of Chaos Adventurer"])
        .library(0, &["Lightning Bolt", "Island", "Island"])
        .dungeon(0, None, 1)
        .at(Step::DeclareAttackers, 0)
        .start();
    let cc = bf(&g, 0, "Caves of Chaos Adventurer");
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    attack(&mut g, &[cc]);
    quiet_until_with(&mut g, |g| g.obj(bolt).zone != Zone::Library && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    let cast = g.obj(bolt).zone == Zone::Graveyard || g.has_option(Choice::Cast(bolt));
    assert!(cast, "free with a dungeon completed\n{}", log(&g));
}

#[test]
fn a_new_nadu_grants_a_new_ability_whose_two_triggers_count_anew_cr_400_7_603_2() {
    // ruling: 96e9a886
    // Skullclamp equipped to the Bear three times: Nadu's granted ability
    // triggers twice (its limit), not a third time. Flickerwisp exiles Nadu;
    // it returns at the end step, a new object: Mother of Runes targeting
    // the Bear then triggers the new Nadu's ability.
    let mut g = scen()
        .battlefield(0, &["Nadu, Winged Wisdom", "Skullclamp", "Test Bear", "Mother of Runes"])
        .battlefield(0, &["Plains"; 6])
        .hand(0, &["Flickerwisp"])
        .library(0, &["Island"; 10])
        .at(Step::Main1, 0)
        .start();
    let nadu = bf(&g, 0, "Nadu, Winged Wisdom");
    let clamp = bf(&g, 0, "Skullclamp");
    let bear = bf(&g, 0, "Test Bear");
    let lib = g.player(0).library.len();
    for _ in 0..3 {
        g.choose(ab(clamp, 0));
        let t = g.tgt(bear);
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    }
    assert_eq!(lib - g.player(0).library.len(), 2, "twice this turn for the Bear\n{}", log(&g));
    let fw = g.card(0, "Flickerwisp", Zone::Hand);
    g.choose(Choice::Cast(fw));
    let tn = g.tgt(nadu);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(tn)).then_some(tn));
    assert_eq!(g.obj(nadu).zone, Zone::Exile, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::End && g.obj(nadu).zone == Zone::Battlefield && at_priority(g) && g.stack().is_empty() && g.turn().priority == 0);
    let before = g.player(0).library.len();
    let m = bf(&g, 0, "Mother of Runes");
    g.choose(ab(m, 0));
    let t = g.tgt(bear);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(3)),
        _ => None,
    });
    assert_eq!(before - g.player(0).library.len(), 1, "the new Nadu's ability triggers\n{}", log(&g));
}
