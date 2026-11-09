//! Planeswalkers (M7b; ENGINE.md 24): CR 306 (loyalty), 606 (loyalty
//! abilities), 120.3c and 704.5i (damage and 0 loyalty), 115.4 (any target),
//! 506.3-506.4 / 508.1b / 508.4 / 510.1b / 702.19 (attacking planeswalkers),
//! 114 (emblems).

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const JACE: &str = "Jace, the Mind Sculptor";

/// Every decision exposed (`auto_single = false`).
fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn loyalty(g: &Game, c: CardIdx) -> u64 {
    g.counters_on(c, Counter::Loyalty)
}

fn tgt_player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

/// Answer the decisions with a single option that aren't priority.
fn settle(g: &mut Game) {
    while let Some(d) = g.pending_decision() {
        if d.kind == DecisionKind::Priority || d.options.len() != 1 {
            return;
        }
        let c = d.options[0].choice;
        g.choose(c);
    }
}

/// Pass priority until the stack is empty (and a player has priority),
/// answering other decisions with `answer` (or their first option).
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

/// Pass priority and decline attacks and blocks until `stop`.
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

/// Cast a card from hand; answer the payment with its first options.
fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    settle(g);
}

/// Answer the rest of a cast or activation (payments) until a player has
/// priority again.
fn pay(g: &mut Game) {
    while !at_priority(g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

#[test]
fn cr_306_5b_a_planeswalker_enters_with_its_printed_loyalty() {
    let mut g = manual().battlefield(0, &["Island"; 4]).hand(0, &[JACE]).at(Step::Main1, 0).start();
    cast(&mut g, 0, JACE);
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let jace = bf(&g, 0, JACE);
    assert_eq!(loyalty(&g, jace), 3);
    assert!(g.is_type(jace, engine::db::types::PLANESWALKER));
}

#[test]
fn cr_606_3_one_loyalty_ability_per_turn_at_sorcery_speed_and_cr_606_4_the_cost_is_paid_on_activation() {
    let mut g = manual().battlefield(0, &[JACE]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let jace = bf(&g, 0, JACE);
    assert_eq!(loyalty(&g, jace), 3);
    // +2, 0 and -1 can be activated; -12 can't (CR 606.6).
    assert!(g.has_option(act(jace, 0)) && g.has_option(act(jace, 1)) && g.has_option(act(jace, 2)));
    assert!(!g.has_option(act(jace, 3)));
    g.choose(act(jace, 0));
    g.choose(tgt_player(1));
    pay(&mut g);
    // CR 606.4: the loyalty cost is paid as the ability is activated.
    assert_eq!(loyalty(&g, jace), 5);
    assert_eq!(g.stack().len(), 1);
    assert!(!g.has_option(act(jace, 1)), "the stack isn't empty, and once per turn");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert!(!g.has_option(act(jace, 1)), "CR 606.3: once each turn");
    assert!(!g.has_option(act(jace, 2)));
    // The opponent's turn: never (sorcery speed in its controller's main
    // phase only).
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 0);
    assert!(!g.has_option(act(jace, 1)));
    // Its controller's next turn: again.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(act(jace, 1)));
    // Not in combat (not a main phase).
    quiet_until(&mut g, |g| g.turn().step == Step::BeginCombat && at_priority(g));
    assert!(!g.has_option(act(jace, 1)));
}

#[test]
fn cr_606_6_a_negative_cost_needs_that_much_loyalty_and_cr_704_5i_zero_loyalty() {
    let base = || {
        manual()
            .battlefield(0, &["Chandra, Torch of Defiance"])
            .battlefield(1, &["Test Bear"])
            .at(Step::Main1, 0)
    };
    let g = base().counters(0, "Chandra, Torch of Defiance", Counter::Loyalty, 2).start();
    let ch = bf(&g, 0, "Chandra, Torch of Defiance");
    assert!(!g.has_option(act(ch, 2)), "-3 with 2 loyalty");
    let mut g = base().counters(0, "Chandra, Torch of Defiance", Counter::Loyalty, 3).start();
    let ch = bf(&g, 0, "Chandra, Torch of Defiance");
    let bear = bf(&g, 1, "Test Bear");
    g.choose(act(ch, 2));
    g.choose(g.tgt(bear));
    pay(&mut g);
    // 0 loyalty: put into its owner's graveyard as P0 receives priority
    // (704.5i); the ability still resolves (113.7a).
    assert_eq!(g.obj(ch).zone, Zone::Graveyard);
    assert_eq!(g.stack().len(), 1);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn cr_115_4_any_target_includes_planeswalkers_and_cr_120_3c_damage_removes_loyalty() {
    // "Target creature" never takes a planeswalker: with only planeswalkers
    // around, Swords to Plowshares can't be cast.
    let g = manual().battlefield(0, &["Plains"]).hand(0, &["Swords to Plowshares"]).battlefield(1, &[JACE]).at(Step::Main1, 0).start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Swords to Plowshares", Zone::Hand))));
    let mut g = manual()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .battlefield(1, &[JACE, "Chandra, Torch of Defiance"])
        .at(Step::Main1, 0)
        .start();
    let jace = bf(&g, 1, JACE);
    let ch = bf(&g, 1, "Chandra, Torch of Defiance");
    cast(&mut g, 0, "Lightning Bolt");
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(jace)) && g.has_option(g.tgt(ch)) && g.has_option(tgt_player(1)));
    g.choose(g.tgt(ch));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(loyalty(&g, ch), 1, "4 - 3");
    assert_eq!(g.life(1), 20);
    cast(&mut g, 0, "Lightning Bolt");
    g.choose(g.tgt(jace));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(jace).zone, Zone::Graveyard, "CR 704.5i");
}

#[test]
fn cr_704_5j_the_legend_rule_for_planeswalkers() {
    let g = manual().battlefield(0, &[JACE, JACE]).at(Step::Main1, 0).start();
    assert_eq!(g.kind(), Some(DecisionKind::LegendRule));
}

/// P0 declares `attacker` as an attacker; P1 controls Jace and `blockers`.
fn attack_jace(attacker: &str, extra: &[&str], blockers: &[&str]) -> Game {
    let mut g = manual()
        .battlefield(0, &[attacker])
        .battlefield(0, extra)
        .battlefield(1, &[JACE])
        .battlefield(1, blockers)
        .at(Step::DeclareAttackers, 0)
        .start();
    let a = bf(&g, 0, attacker);
    g.choose(Choice::Card(a));
    g
}

/// The rest of the attack declaration (`PASS`), up to P0's priority.
fn done_attacking(g: &mut Game) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Pass);
    settle(g);
}

#[test]
fn cr_508_1b_what_each_attacker_attacks_and_cr_510_1b_damage_to_the_planeswalker() {
    let mut g = attack_jace("Test Bear", &[], &[]);
    let bear = bf(&g, 0, "Test Bear");
    let jace = bf(&g, 1, JACE);
    assert_eq!(g.kind(), Some(DecisionKind::AttackTarget));
    assert!(g.has_option(tgt_player(1)) && g.has_option(g.tgt(jace)));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2);
    g.choose(g.tgt(jace));
    done_attacking(&mut g);
    assert_eq!(g.attack_dest(bear), Some(Target::Obj(g.obj_ref(jace))));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(loyalty(&g, jace), 1);
    assert_eq!(g.life(1), 20);
    // Choosing the player: the player is dealt the damage.
    let mut g = attack_jace("Test Bear", &[], &[]);
    g.choose(tgt_player(1));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(loyalty(&g, bf(&g, 1, JACE)), 3);
    assert_eq!(g.life(1), 18);
}

#[test]
fn cr_508_1b_no_attack_target_decision_without_planeswalkers() {
    let mut g = manual().battlefield(0, &["Test Bear", "Test Hasty"]).at(Step::DeclareAttackers, 0).start();
    let bear = bf(&g, 0, "Test Bear");
    g.choose(Choice::Card(bear));
    assert_eq!(g.kind(), Some(DecisionKind::DeclareAttackers));
    assert_eq!(g.attack_dest(bear), Some(Target::Player(1)));
}

#[test]
fn cr_702_19b_trample_assigns_the_excess_to_the_attacked_planeswalker() {
    let mut g = attack_jace("Test Trampler", &[], &["Test Bear"]);
    let tr = bf(&g, 0, "Test Trampler");
    let jace = bf(&g, 1, JACE);
    g.choose(g.tgt(jace));
    done_attacking(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(tr));
    g.drive_until(|g| g.kind() == Some(DecisionKind::AssignDamage));
    // At least lethal (2) to the Bear; the rest to Jace.
    assert!(!g.has_option(Choice::Number(1)));
    g.choose(Choice::Number(2));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(loyalty(&g, jace), 1);
    assert_eq!(g.life(1), 20, "nothing to the player");
}

#[test]
fn cr_506_4c_an_attacker_of_a_removed_planeswalker_deals_no_combat_damage() {
    for trampler in [false, true] {
        let attacker = if trampler { "Test Trampler" } else { "Test Bear" };
        let blockers: &[&str] = if trampler { &["Test Reacher"] } else { &[] };
        let mut g = manual()
            .battlefield(0, &[attacker, "Mountain"])
            .hand(0, &["Lightning Bolt"])
            .battlefield(1, &[JACE])
            .battlefield(1, blockers)
            .at(Step::DeclareAttackers, 0)
            .start();
        let a = bf(&g, 0, attacker);
        let jace = bf(&g, 1, JACE);
        g.choose(Choice::Card(a));
        g.choose(g.tgt(jace));
        done_attacking(&mut g);
        // In the declare attackers step, P0 bolts Jace (3 loyalty).
        assert!(at_priority(&g));
        cast(&mut g, 0, "Lightning Bolt");
        g.choose(g.tgt(jace));
        pay(&mut g);
        resolve_with(&mut g, |_| None);
        assert_eq!(g.obj(jace).zone, Zone::Graveyard);
        assert!(g.obj(a).attacking, "still attacking (CR 506.4c)");
        assert_eq!(g.attack_dest(a), None, "but nothing");
        if trampler {
            let r = bf(&g, 1, "Test Reacher");
            g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
            g.choose(Choice::Card(a));
            quiet_until(&mut g, |g| g.turn().step == Step::Main2);
            // CR 702.19f: all of it to the blocker, none to the player.
            assert_eq!(g.obj(r).zone, Zone::Graveyard);
        } else {
            quiet_until(&mut g, |g| g.turn().step == Step::Main2);
        }
        assert_eq!(g.life(1), 20, "CR 510.1b");
    }
}

#[test]
fn cr_510_3a_combat_damage_to_a_planeswalker_is_not_damage_to_a_player() {
    // Ragavan: "whenever Ragavan deals combat damage to a player".
    let mut g = attack_jace("Ragavan, Nimble Pilferer", &[], &[]);
    let jace = bf(&g, 1, JACE);
    g.choose(g.tgt(jace));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(loyalty(&g, jace), 1);
    assert_eq!(g.count_in(0, "Treasure", Zone::Battlefield), 0, "no trigger");
}

#[test]
fn cr_508_4_a_creature_put_onto_the_battlefield_attacking_chooses_what_it_attacks() {
    // ruling: 7745b178
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Test Bear", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Raph & Mikey, Troublemakers"])
        .battlefield(1, &[JACE])
        .at(Step::DeclareAttackers, 0)
        .start();
    let rm = bf(&g, 0, "Raph & Mikey, Troublemakers");
    let jace = bf(&g, 1, JACE);
    g.choose(Choice::Card(rm));
    g.choose(tgt_player(1));
    done_attacking(&mut g);
    // Raph & Mikey's trigger reveals the Bear: it enters attacking; its
    // controller chooses what it attacks.
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::AttackTarget)).then(|| g.tgt(jace)));
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.obj(bear).attacking);
    assert_eq!(g.attack_dest(bear), Some(Target::Obj(g.obj_ref(jace))));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(loyalty(&g, jace), 1);
    assert_eq!(g.life(1), 20 - 7, "Raph & Mikey's 7");
}

#[test]
fn cr_114_an_emblem_is_in_the_command_zone_its_abilities_function_there() {
    let mut g = manual()
        .emblem(0, "Chandra, Torch of Defiance")
        .battlefield(0, &["Island"])
        .hand(0, &["Consider"])
        .at(Step::Main1, 0)
        .start();
    assert_eq!(g.command_zone().len(), 1);
    let emblem = g.command_zone()[0];
    assert_eq!(g.obj(emblem).zone, Zone::Command);
    assert!(!g.battlefield().contains(&emblem));
    // "Whenever you cast a spell, this emblem deals 5 damage to any target."
    cast(&mut g, 0, "Consider");
    // The trigger goes on the stack as P0 would receive priority; its
    // target is chosen then.
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.stack().len(), 2);
    // CR 114.5: not a permanent; nothing can target it.
    let r = g.obj_ref(emblem);
    assert!(g.pending_decision().unwrap().options.iter().all(|o| o.choice != Choice::Target(Target::Obj(r))));
    g.choose(tgt_player(1));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.life(1), 15);
}

#[test]
fn cr_606_3_the_wandering_emperor_activates_at_instant_speed_the_turn_she_entered() {
    // ruling: 3b688dfd
    let mut g = manual()
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains"])
        .hand(0, &["The Wandering Emperor"])
        .battlefield(1, &["Test Bear"])
        .at(Step::DeclareAttackers, 1)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Card(bear));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Pass);
    // P1 passes in the declare attackers step; P0 flashes the Emperor in.
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    cast(&mut g, 0, "The Wandering Emperor");
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let emp = bf(&g, 0, "The Wandering Emperor");
    assert_eq!(loyalty(&g, emp), 3);
    // P1 has priority after the resolution; P0 gets it on P1's pass.
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(g.has_option(act(emp, 2)), "-2: exile target tapped creature");
    g.choose(act(emp, 2));
    g.choose(g.tgt(bear));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.life(0), 22);
    // Once per turn.
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(!g.has_option(act(emp, 1)));
    // P0's own turn: at sorcery speed only (she didn't enter this turn).
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(act(emp, 1)));
    quiet_until(&mut g, |g| g.turn().step == Step::BeginCombat && at_priority(g));
    assert!(!g.has_option(act(emp, 1)));
}

#[test]
fn cr_205_1a_kaito_is_a_creature_not_a_planeswalker_on_his_turn_and_damage_is_marked() {
    // ruling: f0224bc7
    // "During your turn, ... he's a 3/4 Ninja creature": his card types
    // become exactly creature (CR 205.1a), so he isn't a planeswalker then
    // and damage to him is marked (120.3e), no loyalty lost (120.3c). (Until
    // M8d the engine kept him a planeswalker too: the card's ruling.)
    let mut g = manual()
        .battlefield(0, &["Kaito, Bane of Nightmares", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Kaito, Bane of Nightmares");
    assert!(g.is_creature(k) && !g.is_type(k, engine::db::types::PLANESWALKER));
    assert_eq!(g.pt(k), (3, 4));
    cast(&mut g, 0, "Lightning Bolt");
    g.choose(g.tgt(k));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(loyalty(&g, k), 4);
    assert_eq!(g.obj(k).damage, 3);
    assert_eq!(g.obj(k).zone, Zone::Battlefield);
    // On the opponent's turn he is a planeswalker again (and not a creature).
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1);
    assert!(!g.is_creature(k) && g.is_type(k, engine::db::types::PLANESWALKER));
}
