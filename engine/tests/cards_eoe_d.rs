//! Edge of Eternities, lane D (ENGINE.md 99.5): creatures, combat, removal,
//! damage, life, tokens and pumps. Each test checks a card against its
//! Oracle text; names cite the CR (Comprehensive Rules of 2026-11-06).
//!
//! The card table is the set's own (`CardDb::eoe()`'s cards) plus a few
//! vanilla test cards written as IR, lowered here: never in
//! `tests/common` (the cube's fuzz pool and digests take every card there).

use engine::db::{AbilitySlot, CardDb, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};
use serde_json::{json, Value};
use std::sync::OnceLock;

fn vanilla(name: &str, mana: &str, types: &[&str], p: Option<i64>, t: Option<i64>, extra: Value) -> Value {
    let mut pr = json!({
        "$t": "CardPrinting",
        "name": name,
        "mana_cost_str": mana,
        "mana_cost": {"$t": "ManaCost", "generic": 1},
        "type_line": "Test",
        "supertypes": [],
        "card_types": types,
        "subtypes": [],
    });
    if let (Some(p), Some(t)) = (p, t) {
        pr["power"] = json!(p);
        pr["toughness"] = json!(t);
    }
    if let Value::Object(m) = extra {
        for (k, v) in m {
            pr[k] = v;
        }
    }
    json!({"name": name, "custom": true, "printing": pr})
}

/// "When this enters, <effect>."
fn etb(effect: Value) -> Value {
    json!({"triggered_abilities": [{"$t": "TriggeredAbility", "event": "enters", "predicate": {"$t": "IsSelf"}, "effect": effect}]})
}

/// "{T}: <effect>."
fn act(effect: Value) -> Value {
    json!({"activated_abilities": [{"$t": "ActivatedAbility", "cost": {"$t": "ActivationCost", "tap_self": true}, "effect": effect}]})
}

/// Vanilla test cards (all cost {1}) and lane D's features on test cards.
fn custom_cards() -> Vec<card_ir::Card> {
    let none = json!({});
    let gain = json!({"activated_abilities": [{"$t": "ActivatedAbility",
        "cost": {"$t": "ActivationCost", "tap_self": true},
        "effect": {"$t": "GainLife", "amount": 1}}]});
    let cards = vec![
        vanilla("Test Bear", "{1}", &["Creature"], Some(2), Some(2), none.clone()),
        vanilla("Test Ogre", "{1}", &["Creature"], Some(4), Some(4), none.clone()),
        vanilla("Test Flyer", "{1}", &["Creature"], Some(2), Some(2), json!({"keywords": [{"$t": "Keyword", "kind": "flying"}]})),
        // "{T}: You gain 1 life": a creature its controller can tap.
        vanilla("Test Tapper", "{1}", &["Creature"], Some(1), Some(1), gain),
        vanilla("Test Relic", "{1}", &["Artifact"], None, None, none.clone()),
        vanilla("Test Bulwark", "{1}", &["Creature"], Some(2), Some(5), none.clone()),
        vanilla("Test Sky Wall", "{1}", &["Creature"], Some(2), Some(5), json!({"keywords": [{"$t": "Keyword", "kind": "flying"}]})),
        vanilla("Test Ship", "{1}", &["Artifact"], None, None, json!({"subtypes": ["Spacecraft"]})),
        vanilla("Test Sliver", "{1}", &["Creature"], Some(1), Some(1), json!({"subtypes": ["Sliver"]})),
        // Lane D features whose cards need another lane's too, on test cards.
        vanilla("Test Kill Ship", "{1}", &["Creature"], Some(1), Some(1), etb(json!({"$t": "DealDamageToTargetCreature",
            "amount": 10, "optional": true, "targets": [{"$t": "TargetSpec", "count_min": 0, "filter": {"$t": "IsCreature"}}]}))),
        vanilla("Test Specialist", "{1}", &["Creature"], Some(1), Some(1), etb(json!({"$t": "DealDamageToTargetOpponent",
            "amount": 2, "targets": [{"$t": "TargetSpec", "filter": {"$t": "ObjectAnd", "left": {"$t": "IsPlayer"},
            "right": {"$t": "ControlledByOpponent"}}}]}))),
        vanilla("Test Freighter", "{1}", &["Creature"], Some(1), Some(1),
            etb(json!({"$t": "ReturnUpToNTargetCreaturesToOwnersHands", "exclude_spacecraft": true}))),
        vanilla("Test Seedship", "{1}", &["Creature"], Some(1), Some(1), etb(json!({"$t": "OptionalPutCreatureFromHandOntoBattlefield"}))),
        vanilla("Test Rover", "{1}", &["Creature"], Some(1), Some(1), json!({"triggered_abilities": [{"$t": "TriggeredAbility",
            "event": "dies", "predicate": {"$t": "IsSelf"}, "effect": {"$t": "CreateToken", "each_player": true,
            "token_printing": {"$t": "CardPrinting", "card_types": ["Artifact"], "name": "Test Junk", "subtypes": [],
            "supertypes": ["Token"], "mana_cost": {"$t": "ManaCost"}, "mana_cost_str": "", "type_line": "Token Artifact"}}}]})),
        vanilla("Test Fair", "{1}", &["Artifact"], None, None, json!({"triggered_abilities": [{"$t": "TriggeredAbility",
            "event": "beginning_of_upkeep", "predicate": {"$t": "Controlled"}, "effect": {"$t": "ConditionalIfYouControlAtLeast",
            "threshold": 3, "predicate": "artifact", "inner": {"$t": "GainLife", "amount": 1}}}]})),
        vanilla("Test Starport", "{1}", &["Creature"], Some(1), Some(1), act(json!({"$t": "TapAnotherTargetCreature",
            "targets": [{"$t": "TargetSpec", "filter": {"$t": "ObjectAnd", "left": {"$t": "IsCreature"},
            "right": {"$t": "ObjectNot", "inner": {"$t": "IsSource"}}}}]}))),
        vanilla("Test Harvester", "{1}", &["Creature"], Some(3), Some(3), act(json!({"$t": "DealDamageEqualToSourcePowerAnyTarget",
            "targets": [{"$t": "TargetSpec", "filter": {"$t": "OneOf", "filters": [{"$t": "IsCreature"}, {"$t": "IsPlayer"}]}}]}))),
        vanilla("Test Overlord", "{1}", &["Creature"], Some(1), Some(1), act(json!({"$t": "GainControlOfTargetSliverPermanently"}))),
        vanilla("Test Reroute", "{1}", &["Instant"], None, None, json!({"spell_effect": {"$t": "DealDamageToTargetTappedCreature",
            "amount": 2, "targets": [{"$t": "TargetSpec", "filter": {"$t": "ObjectAnd", "left": {"$t": "IsCreature"},
            "right": {"$t": "IsTapped"}}}]}})),
        vanilla("Test Vote Out", "{1}", &["Sorcery"], None, None, json!({"spell_effect": {"$t": "DestroyTargetCreature",
            "targets": [{"$t": "TargetSpec", "filter": {"$t": "IsCreature"}}]}})),
        vanilla("Test Oblivion", "{1}", &["Sorcery"], None, None, json!({"spell_effect": {"$t": "DestroyTargetCreature",
            "or_spacecraft": true, "targets": [{"$t": "TargetSpec", "filter": {"$t": "OneOf", "filters": [{"$t": "IsCreature"},
            {"$t": "HasSubtype", "name": "Spacecraft"}]}}]}})),
    ];
    cards.into_iter().map(|v| serde_json::from_value(v).expect("test card IR")).collect()
}

fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_eoe_document().cards;
        cards.extend(custom_cards());
        let db = CardDb::from_cards(&cards);
        for c in custom_cards() {
            db.expect_id(&c.name).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
    })
}

/// Every decision answered by the test (no automatic single options).
fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Plains"; 10]).library(1, &["Plains"; 10])
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn all_bf(g: &Game, p: PlayerId, name: &str) -> Vec<CardIdx> {
    g.battlefield().iter().copied().filter(|&c| g.obj(c).controller == p && g.name(c) == name).collect()
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

/// Answer decisions until `stop`: `answer` first, else pass priority,
/// attacks and blocks, and take the first option of anything else.
fn run(g: &mut Game, stop: impl Fn(&Game) -> bool, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("game over\n{}", log(g)));
        let c = answer(g).unwrap_or(match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        });
        g.choose(c);
    }
    panic!("run: no progress\n{}", log(g));
}

/// Resolve the stack (answering with `answer`, else the first option).
fn settle_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    run(g, |g| g.stack().is_empty() && at_priority(g), answer);
}

fn settle(g: &mut Game) {
    settle_with(g, |_| None);
}

/// Cast `name` from P0's hand and pay with the first sources offered.
fn cast(g: &mut Game, name: &str) {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// Activate `n` Test Tappers of `p` (tapping them; `p` keeps priority),
/// resolve, and give `p` priority again.
fn tap_tappers(g: &mut Game, p: PlayerId, n: usize) {
    for c in all_bf(g, p, "Test Tapper").into_iter().take(n) {
        g.choose(Choice::Ability(c, AbilitySlot::Activated(0)));
    }
    settle(g);
    if g.to_move() != Some(p) {
        g.pass();
    }
    assert!(g.to_move() == Some(p) && at_priority(g));
}

/// Until the cleanup step of the current turn (the end step's triggers
/// resolved), answering with `answer`.
fn through_end_step(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    let t = g.turn().number;
    run(g, |g| g.turn().number > t || g.turn().step == Step::Cleanup, answer);
}

/// Until P`p`'s next precombat main phase with priority.
fn next_main(g: &mut Game, p: PlayerId) {
    let t = g.turn().number;
    run(g, |g| g.turn().number > t && g.turn().active == p && g.turn().step == Step::Main1 && at_priority(g), |_| None);
}

/// Declare `names` of P`p` as attackers (the active player) and continue
/// to the next decision.
fn attack(g: &mut Game, p: PlayerId, names: &[&str]) {
    run(g, |g| g.kind() == Some(DecisionKind::DeclareAttackers), |_| None);
    for n in names {
        let c = bf(g, p, n);
        g.choose(Choice::Card(c));
    }
    g.choose(Choice::Pass);
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

// ---- "if you control two or more tapped creatures" (CR 603.4) ----------

#[test]
fn cr_603_4_flight_deck_coordinator_gains_two_life_with_two_tapped_creatures() {
    let mut g = scen().battlefield(0, &["Flight-Deck Coordinator", "Test Tapper", "Test Tapper"]).at(Step::Main1, 0).start();
    tap_tappers(&mut g, 0, 2);
    assert_eq!(g.life(0), 22);
    through_end_step(&mut g, |_| None);
    assert_eq!(g.life(0), 24, "{}", log(&g));
}

#[test]
fn cr_603_4_the_intervening_if_with_one_tapped_creature_does_not_trigger() {
    let mut g = scen().battlefield(0, &["Flight-Deck Coordinator", "Test Tapper", "Test Tapper"]).at(Step::Main1, 0).start();
    tap_tappers(&mut g, 0, 1);
    through_end_step(&mut g, |_| None);
    assert_eq!(g.life(0), 21);
    assert!(!log(&g).contains("trigger of Flight-Deck Coordinator"), "{}", log(&g));
}

#[test]
fn cr_603_4_dawnstrike_vanguard_puts_a_counter_on_each_other_creature() {
    let mut g = scen().battlefield(0, &["Dawnstrike Vanguard", "Test Tapper", "Test Tapper"]).at(Step::Main1, 0).start();
    tap_tappers(&mut g, 0, 2);
    through_end_step(&mut g, |_| None);
    for c in all_bf(&g, 0, "Test Tapper") {
        assert_eq!(g.counters_on(c, Counter::P1P1), 1);
    }
    assert_eq!(g.counters_on(bf(&g, 0, "Dawnstrike Vanguard"), Counter::P1P1), 0, "other than this creature");
}

#[test]
fn cr_603_4_frontline_war_rager_puts_a_counter_on_itself() {
    let mut g = scen().battlefield(0, &["Frontline War-Rager", "Test Tapper", "Test Tapper"]).at(Step::Main1, 0).start();
    tap_tappers(&mut g, 0, 2);
    through_end_step(&mut g, |_| None);
    assert_eq!(g.counters_on(bf(&g, 0, "Frontline War-Rager"), Counter::P1P1), 1);
    for c in all_bf(&g, 0, "Test Tapper") {
        assert_eq!(g.counters_on(c, Counter::P1P1), 0);
    }
}

#[test]
fn cr_603_4_sami_creates_a_tapped_robot_token() {
    let mut g = scen().battlefield(0, &["Sami, Ship's Engineer", "Test Tapper", "Test Tapper"]).at(Step::Main1, 0).start();
    tap_tappers(&mut g, 0, 2);
    through_end_step(&mut g, |_| None);
    let robots = all_bf(&g, 0, "Robot");
    assert_eq!(robots.len(), 1, "{}", log(&g));
    let r = robots[0];
    assert!(g.obj(r).tapped);
    assert_eq!((g.power(r), g.toughness(r)), (2, 2));
    assert!(g.def_of(r).is(engine::db::types::ARTIFACT) && g.def_of(r).is(engine::db::types::CREATURE));
}

#[test]
fn cr_603_4_vaultguard_trooper_may_discard_the_hand_to_draw_two() {
    // Yes: the hand (two cards) is discarded, two cards drawn.
    let mut g = scen()
        .battlefield(0, &["Vaultguard Trooper", "Test Tapper", "Test Tapper"])
        .hand(0, &["Test Bear", "Test Ogre"])
        .at(Step::Main1, 0)
        .start();
    tap_tappers(&mut g, 0, 2);
    through_end_step(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard) + g.count_in(0, "Test Ogre", Zone::Graveyard), 2);
    assert_eq!(g.player(0).hand.len(), 2);
    assert_eq!(g.count_in(0, "Plains", Zone::Hand), 2);
    // No: nothing happens.
    let mut g = scen()
        .battlefield(0, &["Vaultguard Trooper", "Test Tapper", "Test Tapper"])
        .hand(0, &["Test Bear", "Test Ogre"])
        .at(Step::Main1, 0)
        .start();
    tap_tappers(&mut g, 0, 2);
    let mut asked = false;
    for _ in 0..1000 {
        if g.turn().step == Step::Cleanup || g.turn().number > 3 {
            break;
        }
        if g.kind() == Some(DecisionKind::YesNo) {
            asked = true;
            g.choose(Choice::Pass);
            continue;
        }
        run(&mut g, |g| g.kind() == Some(DecisionKind::YesNo) || g.turn().step == Step::Cleanup || g.turn().number > 3, |_| None);
    }
    assert!(asked, "the discard is a choice");
    assert_eq!(g.count_in(0, "Test Bear", Zone::Hand) + g.count_in(0, "Test Ogre", Zone::Hand), 2);
    assert_eq!(g.player(0).hand.len(), 2);
}

// ---- Memorial Team Leader: "During your turn" (CR 611.3a) ---------------

#[test]
fn cr_611_3a_memorial_team_leader_pumps_other_creatures_during_your_turn_only() {
    let g = scen().battlefield(0, &["Memorial Team Leader", "Test Bear"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    assert_eq!(g.power(bf(&g, 0, "Test Bear")), 3);
    assert_eq!(g.power(bf(&g, 0, "Memorial Team Leader")), 4, "other creatures");
    assert_eq!(g.power(bf(&g, 1, "Test Bear")), 2, "creatures you control");
    let g = scen().battlefield(0, &["Memorial Team Leader", "Test Bear"]).at(Step::Main1, 1).start();
    assert_eq!(g.power(bf(&g, 0, "Test Bear")), 2, "not during the opponent's turn");
}

// ---- Nova Hellkite: 1 damage to target creature an opponent controls -----

#[test]
fn cr_120_3e_nova_hellkite_deals_one_damage_to_an_opponents_creature() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", "Mountain", "Test Bear"])
        .battlefield(1, &["Test Bear", "Test Ogre"])
        .hand(0, &["Nova Hellkite"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Nova Hellkite");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    let (mine, theirs) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Bear"));
    let opts = options(&g);
    assert!(!opts.contains(&g.tgt(mine)) && !opts.contains(&g.tgt(bf(&g, 0, "Nova Hellkite"))), "an opponent's creature");
    g.choose(g.tgt(theirs));
    settle(&mut g);
    assert_eq!(g.obj(theirs).damage, 1);
}

// ---- Exosuit Savior: return up to one other target permanent you control --

#[test]
fn cr_115_10_exosuit_savior_returns_up_to_one_other_permanent_you_control() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Test Bear"])
        .battlefield(1, &["Test Ogre"])
        .hand(0, &["Exosuit Savior"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Exosuit Savior");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    let opts = options(&g);
    assert!(opts.contains(&Choice::Pass), "up to one");
    assert!(!opts.contains(&g.tgt(bf(&g, 1, "Test Ogre"))), "you control");
    assert!(!opts.contains(&g.tgt(bf(&g, 0, "Exosuit Savior"))), "other");
    assert!(opts.contains(&g.tgt(bf(&g, 0, "Plains"))), "any permanent");
    g.choose(g.tgt(bf(&g, 0, "Test Bear")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Hand), 1);
}

// ---- Mechanozoa: tap and stun (CR 701.26a, 122.1d) ------------------------

#[test]
fn cr_122_1d_mechanozoa_taps_and_stuns_an_opponents_artifact_or_creature() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island", "Test Relic"])
        .battlefield(1, &["Test Bear", "Test Relic"])
        .hand(0, &["Mechanozoa"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Mechanozoa");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    let bear = bf(&g, 1, "Test Bear");
    let opts = options(&g);
    assert!(opts.contains(&g.tgt(bf(&g, 1, "Test Relic"))), "an artifact");
    assert!(!opts.contains(&g.tgt(bf(&g, 0, "Test Relic"))), "an opponent's");
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert!(g.obj(bear).tapped);
    assert_eq!(g.counters_on(bear, Counter::Stun), 1);
    // Its untap step: the stun counter is removed instead (CR 122.1d).
    next_main(&mut g, 1);
    assert!(g.obj(bear).tapped);
    assert_eq!(g.counters_on(bear, Counter::Stun), 0);
}

// ---- Swarm Culler and Comet Crawler: "you may sacrifice another creature or
// artifact. If you do, ..." (CR 701.21a) -----------------------------------

#[test]
fn cr_701_21a_swarm_culler_may_sacrifice_another_to_draw_when_it_becomes_tapped() {
    let mut g = scen().battlefield(0, &["Swarm Culler", "Test Bear"]).at(Step::Main1, 0).start();
    attack(&mut g, 0, &["Swarm Culler"]);
    run(&mut g, |g| g.kind() == Some(DecisionKind::Sacrifice) || g.kind() == Some(DecisionKind::ChoosePermanent), |_| None);
    let culler = bf(&g, 0, "Swarm Culler");
    assert!(!options(&g).contains(&Choice::Card(culler)), "another");
    assert!(options(&g).contains(&Choice::Pass), "may");
    let hand = g.player(0).hand.len();
    g.choose(Choice::Card(bf(&g, 0, "Test Bear")));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.player(0).hand.len(), hand + 1);
}

#[test]
fn cr_701_21a_comet_crawler_may_sacrifice_an_artifact_for_plus_two() {
    let mut g = scen().battlefield(0, &["Comet Crawler", "Test Relic"]).at(Step::Main1, 0).start();
    attack(&mut g, 0, &["Comet Crawler"]);
    run(&mut g, |g| g.kind() == Some(DecisionKind::Sacrifice) || g.kind() == Some(DecisionKind::ChoosePermanent), |_| None);
    g.choose(Choice::Card(bf(&g, 0, "Test Relic")));
    settle(&mut g);
    let c = bf(&g, 0, "Comet Crawler");
    assert_eq!(g.power(c), 4);
    run(&mut g, |g| g.turn().step == Step::EndCombat, |_| None);
    assert_eq!(g.life(1), 16);
    assert_eq!(g.life(0), 24, "lifelink");
    // Declining: no bonus.
    let mut g = scen().battlefield(0, &["Comet Crawler", "Test Relic"]).at(Step::Main1, 0).start();
    attack(&mut g, 0, &["Comet Crawler"]);
    run(&mut g, |g| g.kind() == Some(DecisionKind::Sacrifice) || g.kind() == Some(DecisionKind::ChoosePermanent), |_| None);
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.power(bf(&g, 0, "Comet Crawler")), 2);
    assert_eq!(g.count_in(0, "Test Relic", Zone::Battlefield), 1);
}

// ---- Zealous Display: "If it's not your turn, untap those creatures" ------

#[test]
fn cr_701_26b_zealous_display_untaps_only_during_an_opponents_turn() {
    // The opponent's turn: pumped and untapped.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Test Tapper", "Test Tapper"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Zealous Display"])
        .at(Step::Main1, 1)
        .start();
    g.pass(); // P1 passes: P0's priority
    assert_eq!(g.to_move(), Some(0));
    tap_tappers(&mut g, 0, 2);
    cast(&mut g, "Zealous Display");
    settle(&mut g);
    for c in all_bf(&g, 0, "Test Tapper") {
        assert!(!g.obj(c).tapped, "untapped");
        assert_eq!(g.power(c), 3);
    }
    assert_eq!(g.power(bf(&g, 1, "Test Bear")), 2, "creatures you control");
    // Its controller's own turn: pumped, still tapped.
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Test Tapper", "Test Tapper"])
        .hand(0, &["Zealous Display"])
        .at(Step::Main1, 0)
        .start();
    tap_tappers(&mut g, 0, 2);
    cast(&mut g, "Zealous Display");
    settle(&mut g);
    for c in all_bf(&g, 0, "Test Tapper") {
        assert!(g.obj(c).tapped);
        assert_eq!(g.power(c), 3);
    }
}

// ---- Depressurize: -3/-0, then destroy if its power is 0 or less ----------

#[test]
fn cr_701_8a_depressurize_destroys_a_creature_whose_power_drops_to_zero_or_less() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Depressurize"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Depressurize");
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1, "{}", log(&g));
    // Power 4: 1/4 until end of turn, not destroyed.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .battlefield(1, &["Test Ogre"])
        .hand(0, &["Depressurize"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Depressurize");
    settle(&mut g);
    let o = bf(&g, 1, "Test Ogre");
    assert_eq!((g.power(o), g.toughness(o)), (1, 4));
}

// ---- Biosynthic Burst ------------------------------------------------------

#[test]
fn cr_701_26b_biosynthic_burst_counter_keywords_and_untap() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Test Tapper"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Biosynthic Burst"])
        .at(Step::Main1, 0)
        .start();
    tap_tappers(&mut g, 0, 1);
    cast(&mut g, "Biosynthic Burst");
    run(&mut g, |g| g.stack().is_empty() && at_priority(g), |g| {
        // A creature you control.
        (g.kind() == Some(DecisionKind::Target)).then(|| {
            assert!(!options(g).contains(&g.tgt(bf(g, 1, "Test Bear"))));
            g.tgt(bf(g, 0, "Test Tapper"))
        })
    });
    let t = bf(&g, 0, "Test Tapper");
    assert_eq!(g.counters_on(t, Counter::P1P1), 1);
    assert!(g.has_kw(t, Kw::Reach) && g.has_kw(t, Kw::Trample) && g.has_kw(t, Kw::Indestructible));
    assert!(!g.obj(t).tapped);
    // Until end of turn: the keywords end in the cleanup step (CR 514.2).
    next_main(&mut g, 1);
    assert!(!g.has_kw(t, Kw::Trample));
    assert_eq!(g.counters_on(t, Counter::P1P1), 1);
}

// ---- Genemorph Imago: landfall, base 3/3 or 6/6 (CR 613.4b) ---------------

#[test]
fn cr_613_4b_genemorph_imago_sets_base_three_three_or_six_six_with_six_lands() {
    for (lands, pt) in [(4usize, (3, 3)), (5, (6, 6))] {
        let mut g = scen()
            .battlefield(0, &vec!["Island"; lands])
            .battlefield(0, &["Genemorph Imago"])
            .hand(0, &["Forest"])
            .at(Step::Main1, 0)
            .start();
        let f = g.card(0, "Forest", Zone::Hand);
        g.choose(Choice::Play(f));
        settle(&mut g);
        let c = bf(&g, 0, "Genemorph Imago");
        assert_eq!((g.power(c), g.toughness(c)), pt, "{lands} lands + 1\n{}", log(&g));
    }
}

// ---- Kavaron Harrier: an attacking token sacrificed at end of combat ------

#[test]
fn cr_511_2_kavaron_harriers_robot_attacks_then_is_sacrificed_at_end_of_combat() {
    let mut g = scen().battlefield(0, &["Kavaron Harrier", "Mountain", "Mountain"]).at(Step::Main1, 0).start();
    attack(&mut g, 0, &["Kavaron Harrier"]);
    run(&mut g, |g| g.kind() == Some(DecisionKind::YesNo), |_| None);
    g.choose(Choice::Yes);
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    settle(&mut g);
    let robot = bf(&g, 0, "Robot");
    assert!(g.obj(robot).tapped && g.obj(robot).attacking);
    run(&mut g, |g| g.turn().step == Step::EndCombat, |_| None);
    assert_eq!(g.life(1), 16, "both attacked");
    run(&mut g, |g| g.turn().step == Step::Main2, |_| None);
    assert!(all_bf(&g, 0, "Robot").is_empty(), "sacrificed at end of combat\n{}", log(&g));
}

// ---- Skystinger: "Whenever this creature blocks a creature with flying" ---

#[test]
fn cr_509_3b_skystinger_gets_plus_five_blocking_a_flyer() {
    for (attacker, power) in [("Test Flyer", 8), ("Test Bear", 3)] {
        let mut g = scen().battlefield(0, &["Skystinger"]).battlefield(1, &[attacker]).at(Step::Main1, 1).start();
        attack(&mut g, 1, &[attacker]);
        run(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers), |_| None);
        let a = bf(&g, 1, attacker);
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::DeclareBlockers) {
            g.choose(Choice::Pass);
        }
        settle(&mut g);
        assert_eq!(g.turn().step, Step::DeclareBlockers);
        assert_eq!(g.power(bf(&g, 0, "Skystinger")), power, "blocking {attacker}\n{}", log(&g));
    }
}


/// Answer the pending Target decision with `c`.
fn target(g: &mut Game, c: CardIdx) {
    run(g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    let t = g.tgt(c);
    g.choose(t);
}

// ---- Faller's Faithful: "If that creature wasn't dealt damage this turn,
// its controller draws two cards" (CR 701.8a, 120.3e) ------------------------

fn fallers(p1: &[&str]) -> Game {
    scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", "Mountain", "Swamp", "Swamp", "Swamp"])
        .battlefield(1, p1)
        .hand(0, &["Faller's Faithful", "Nova Hellkite"])
        .at(Step::Main1, 0)
        .start()
}

#[test]
fn cr_701_8a_fallers_faithful_destroys_and_the_undamaged_creatures_controller_draws_two() {
    let mut g = fallers(&["Test Ogre"]);
    let hand = g.player(1).hand.len();
    cast(&mut g, "Faller's Faithful");
    let ogre = bf(&g, 1, "Test Ogre");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    assert!(options(&g).contains(&Choice::Pass), "up to one");
    assert!(!options(&g).contains(&g.tgt(bf(&g, 0, "Faller's Faithful"))), "other");
    g.choose(g.tgt(ogre));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Ogre", Zone::Graveyard), 1);
    assert_eq!(g.player(1).hand.len(), hand + 2, "its controller draws\n{}", log(&g));
}

#[test]
fn cr_120_3e_fallers_faithful_no_draw_for_a_creature_dealt_damage_this_turn() {
    let mut g = fallers(&["Test Ogre", "Test Bear"]);
    cast(&mut g, "Nova Hellkite");
    let ogre = bf(&g, 1, "Test Ogre");
    target(&mut g, ogre);
    settle(&mut g);
    assert_eq!(g.obj(ogre).damage, 1);
    let hand = g.player(1).hand.len();
    cast(&mut g, "Faller's Faithful");
    target(&mut g, ogre);
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Ogre", Zone::Graveyard), 1);
    assert_eq!(g.player(1).hand.len(), hand, "it was dealt damage this turn");
}

#[test]
fn cr_701_8a_fallers_faithful_an_indestructible_creatures_controller_still_draws() {
    // Biosynthic Burst makes P0's own Bear indestructible; Faller's Faithful
    // targets it: not destroyed, P0 draws two (it wasn't dealt damage).
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Forest", "Forest", "Test Bear"])
        .hand(0, &["Faller's Faithful", "Biosynthic Burst"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    cast(&mut g, "Biosynthic Burst");
    settle_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bear)));
    let hand = g.player(0).hand.len();
    cast(&mut g, "Faller's Faithful");
    target(&mut g, bear);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.player(0).hand.len(), hand - 1 + 2, "cast (-1), drew two");
}

#[test]
fn cr_120_3e_damage_in_an_earlier_turn_is_not_this_turn() {
    let mut g = fallers(&["Test Ogre"]);
    cast(&mut g, "Nova Hellkite");
    let ogre = bf(&g, 1, "Test Ogre");
    target(&mut g, ogre);
    settle(&mut g);
    next_main(&mut g, 0);
    let hand = g.player(1).hand.len();
    cast(&mut g, "Faller's Faithful");
    target(&mut g, ogre);
    settle(&mut g);
    assert_eq!(g.player(1).hand.len(), hand + 2, "dealt damage two turns ago");
}

// ---- Cut Propulsion and Diplomatic Relations: a creature deals damage
// equal to its power (CR 120.3) ---------------------------------------------

#[test]
fn cr_120_3_cut_propulsion_deals_damage_to_itself_twice_with_flying() {
    for (name, dmg) in [("Test Bulwark", 2), ("Test Sky Wall", 4)] {
        let mut g = scen().battlefield(0, &["Mountain", "Mountain", "Mountain"]).battlefield(1, &[name]).hand(0, &["Cut Propulsion"]).at(Step::Main1, 0).start();
        let c = bf(&g, 1, name);
        cast(&mut g, "Cut Propulsion");
        settle(&mut g);
        assert_eq!(g.obj(c).damage, dmg, "{name}\n{}", log(&g));
    }
    let mut g = scen().battlefield(0, &["Mountain", "Mountain", "Mountain"]).battlefield(1, &["Test Bear"]).hand(0, &["Cut Propulsion"]).at(Step::Main1, 0).start();
    cast(&mut g, "Cut Propulsion");
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
}

#[test]
fn cr_120_3_diplomatic_relations_pumps_then_deals_damage_equal_to_power() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Test Bear"])
        .battlefield(1, &["Test Bulwark", "Test Ogre"])
        .hand(0, &["Diplomatic Relations"])
        .at(Step::Main1, 0)
        .start();
    let (bear, wall) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Bulwark"));
    cast(&mut g, "Diplomatic Relations");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    assert!(!options(&g).contains(&g.tgt(wall)), "a creature you control");
    g.choose(g.tgt(bear));
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    assert!(!options(&g).contains(&g.tgt(bear)), "a creature an opponent controls");
    g.choose(g.tgt(wall));
    settle(&mut g);
    assert_eq!(g.power(bear), 3);
    assert!(g.has_kw(bear, Kw::Vigilance));
    assert_eq!(g.obj(wall).damage, 3, "its power after the pump");
    assert_eq!(g.obj(bear).damage, 0, "not a fight");
}

// ---- Focus Fire: X = 2 + creatures and/or Spacecraft you control ----------

#[test]
fn cr_509_1g_focus_fire_hits_an_attacking_or_blocking_creature_for_two_plus_your_count() {
    // P1 attacks with a 2/5; P0 controls a creature and a Spacecraft: 4.
    let mut g = scen()
        .battlefield(0, &["Plains", "Test Bear", "Test Ship"])
        .battlefield(1, &["Test Bulwark", "Test Ogre"])
        .hand(0, &["Focus Fire"])
        .at(Step::Main1, 1)
        .start();
    attack(&mut g, 1, &["Test Bulwark"]);
    run(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers), |_| None);
    g.choose(Choice::Pass);
    run(&mut g, |g| g.to_move() == Some(0) && at_priority(g), |_| None);
    cast(&mut g, "Focus Fire");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    assert!(!options(&g).contains(&g.tgt(bf(&g, 1, "Test Ogre"))), "attacking or blocking");
    let wall = bf(&g, 1, "Test Bulwark");
    g.choose(g.tgt(wall));
    settle(&mut g);
    assert_eq!(g.obj(wall).damage, 4, "{}", log(&g));
    // A blocking creature is a legal target too.
    let mut g = scen()
        .battlefield(0, &["Plains", "Test Ogre"])
        .battlefield(1, &["Test Bulwark"])
        .hand(0, &["Focus Fire"])
        .at(Step::Main1, 0)
        .start();
    attack(&mut g, 0, &["Test Ogre"]);
    run(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers), |_| None);
    let ogre = bf(&g, 0, "Test Ogre");
    g.choose(Choice::Card(ogre));
    if g.kind() == Some(DecisionKind::DeclareBlockers) {
        g.choose(Choice::Pass);
    }
    run(&mut g, |g| g.to_move() == Some(0) && at_priority(g), |_| None);
    cast(&mut g, "Focus Fire");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    let wall = bf(&g, 1, "Test Bulwark");
    assert!(options(&g).contains(&g.tgt(wall)), "blocking");
    g.choose(g.tgt(wall));
    settle(&mut g);
    assert_eq!(g.obj(wall).damage, 3, "2 plus one creature");
}

// ---- P/T by a number: Hemosymbic Mite, Mightform Harmonizer, Monoist
// Circuit-Feeder (CR 611.2c, 701.10b) ---------------------------------------

#[test]
fn cr_608_2h_hemosymbic_mite_pumps_another_creature_by_its_power() {
    let mut g = scen().battlefield(0, &["Hemosymbic Mite", "Test Bear"]).at(Step::Main1, 0).start();
    attack(&mut g, 0, &["Hemosymbic Mite"]);
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    let mite = bf(&g, 0, "Hemosymbic Mite");
    assert!(!options(&g).contains(&g.tgt(mite)), "another");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert_eq!((g.power(bear), g.toughness(bear)), (3, 3));
}

#[test]
fn cr_701_10b_mightform_harmonizer_doubles_power() {
    let mut g = scen().battlefield(0, &["Mightform Harmonizer", "Test Ogre"]).hand(0, &["Forest"]).at(Step::Main1, 0).start();
    let f = g.card(0, "Forest", Zone::Hand);
    g.choose(Choice::Play(f));
    let ogre = bf(&g, 0, "Test Ogre");
    target(&mut g, ogre);
    settle(&mut g);
    assert_eq!((g.power(ogre), g.toughness(ogre)), (8, 4));
}

#[test]
fn cr_611_2c_monoist_circuit_feeder_x_is_your_artifacts() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Test Relic", "Test Bear"])
        .battlefield(1, &["Test Ogre"])
        .hand(0, &["Monoist Circuit-Feeder"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Monoist Circuit-Feeder");
    let (bear, ogre) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Ogre"));
    target(&mut g, bear);
    target(&mut g, ogre);
    settle(&mut g);
    // Two artifacts: the Relic and the Circuit-Feeder itself.
    assert_eq!((g.power(bear), g.toughness(bear)), (4, 2));
    assert_eq!((g.power(ogre), g.toughness(ogre)), (4, 2));
}

// ---- Mouth of the Storm: until your next turn (CR 611.2b) ------------------

#[test]
fn cr_611_2b_mouth_of_the_storm_shrinks_opponents_creatures_until_your_next_turn() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island", "Island", "Test Bear"])
        .battlefield(1, &["Test Ogre"])
        .hand(0, &["Mouth of the Storm"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Mouth of the Storm");
    settle(&mut g);
    let (bear, ogre) = (bf(&g, 0, "Test Bear"), bf(&g, 1, "Test Ogre"));
    assert_eq!(g.power(ogre), 1);
    assert_eq!(g.power(bear), 2, "your opponents' creatures");
    next_main(&mut g, 1);
    assert_eq!(g.power(ogre), 1, "still during their turn");
    next_main(&mut g, 0);
    assert_eq!(g.power(ogre), 4, "ended as your next turn began");
}

// ---- Perigee Beckoner: a granted "when this creature dies" ----------------

#[test]
fn cr_603_10a_perigee_beckoners_creature_returns_tapped_when_it_dies_this_turn() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Mountain", "Mountain", "Mountain", "Hemosymbic Mite"])
        .hand(0, &["Perigee Beckoner", "Cut Propulsion"])
        .at(Step::Main1, 0)
        .start();
    let mite = bf(&g, 0, "Hemosymbic Mite");
    cast(&mut g, "Perigee Beckoner");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    assert!(!options(&g).contains(&g.tgt(bf(&g, 0, "Perigee Beckoner"))), "another");
    g.choose(g.tgt(mite));
    settle(&mut g);
    assert_eq!(g.power(mite), 3);
    cast(&mut g, "Cut Propulsion");
    target(&mut g, mite);
    settle(&mut g);
    let back = bf(&g, 0, "Hemosymbic Mite");
    assert!(g.obj(back).tapped, "returned tapped\n{}", log(&g));
    // It entered tapped: it never became tapped (no Mite trigger).
    assert!(!log(&g).contains("trigger of Hemosymbic Mite"), "{}", log(&g));
    assert_eq!(g.power(back), 1, "a new object (CR 400.7)");
}

#[test]
fn cr_514_2_perigee_beckoners_granted_ability_ends_with_the_turn() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp", "Swamp", "Mountain", "Mountain", "Mountain", "Test Bear"])
        .hand(0, &["Perigee Beckoner", "Cut Propulsion"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    cast(&mut g, "Perigee Beckoner");
    target(&mut g, bear);
    settle(&mut g);
    next_main(&mut g, 0);
    cast(&mut g, "Cut Propulsion");
    target(&mut g, bear);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1, "stays dead");
}

// ---- The Endstone: "your life total becomes half your starting life total,
// rounded up" (CR 119.5) ----------------------------------------------------

#[test]
fn cr_119_5_the_endstone_sets_life_to_half_starting_and_draws_on_land_and_spell() {
    for (life, after) in [(30, 10), (4, 10)] {
        let mut g = scen().battlefield(0, &["The Endstone"]).hand(0, &["Plains"]).life(0, life).at(Step::Main1, 0).start();
        let hand = g.player(0).hand.len();
        let land = g.card(0, "Plains", Zone::Hand);
        g.choose(Choice::Play(land));
        settle(&mut g);
        assert_eq!(g.player(0).hand.len(), hand, "played a land (-1), drew a card (+1)");
        run(&mut g, |g| g.life(0) != life || g.turn().number > 3, |_| None);
        assert_eq!(g.turn().step, Step::End);
        assert_eq!(g.life(0), after);
        assert_eq!((g.player(0).gained_life, g.player(0).lost_life), (life < 10, life > 10), "the difference is gained or lost
{}", log(&g));
    }
}

// ---- Command Bridge: sacrifice it unless you tap an untapped permanent ----

#[test]
fn cr_118_12_command_bridge_is_sacrificed_unless_you_tap_an_untapped_permanent() {
    // Tap a creature: the land stays.
    let mut g = scen().battlefield(0, &["Test Bear"]).hand(0, &["Command Bridge"]).at(Step::Main1, 0).start();
    let land = g.card(0, "Command Bridge", Zone::Hand);
    g.choose(Choice::Play(land));
    run(&mut g, |g| g.kind() == Some(DecisionKind::ChoosePermanent), |_| None);
    let bridge = bf(&g, 0, "Command Bridge");
    assert!(g.obj(bridge).tapped, "enters tapped");
    assert!(!options(&g).contains(&Choice::Card(bridge)), "untapped permanents");
    g.choose(Choice::Card(bf(&g, 0, "Test Bear")));
    settle(&mut g);
    assert!(g.obj(bf(&g, 0, "Test Bear")).tapped);
    assert_eq!(g.count_in(0, "Command Bridge", Zone::Battlefield), 1);
    // Decline: sacrificed.
    let mut g = scen().battlefield(0, &["Test Bear"]).hand(0, &["Command Bridge"]).at(Step::Main1, 0).start();
    let land = g.card(0, "Command Bridge", Zone::Hand);
    g.choose(Choice::Play(land));
    run(&mut g, |g| g.kind() == Some(DecisionKind::ChoosePermanent), |_| None);
    g.choose(Choice::Pass);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Command Bridge", Zone::Graveyard), 1);
    // No untapped permanent: sacrificed.
    let mut g = scen().hand(0, &["Command Bridge"]).at(Step::Main1, 0).start();
    let land = g.card(0, "Command Bridge", Zone::Hand);
    g.choose(Choice::Play(land));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Command Bridge", Zone::Graveyard), 1);
}

// ---- Lane D features on test cards (their EoE cards need another lane) ----

#[test]
fn cr_115_10_up_to_one_target_creature_damage_and_target_opponent_damage() {
    // "Deals 10 damage to up to one target creature" (Pinnacle Kill-Ship).
    let mut g = scen().battlefield(0, &["Plains"]).battlefield(1, &["Test Ogre"]).hand(0, &["Test Kill Ship"]).at(Step::Main1, 0).start();
    cast(&mut g, "Test Kill Ship");
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    assert!(options(&g).contains(&Choice::Pass), "up to one");
    g.choose(g.tgt(bf(&g, 1, "Test Ogre")));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Ogre", Zone::Graveyard), 1);
    // "Deals 2 damage to target opponent" (Biotech Specialist).
    let mut g = scen().battlefield(0, &["Plains"]).hand(0, &["Test Specialist"]).at(Step::Main1, 0).start();
    cast(&mut g, "Test Specialist");
    settle_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::Target)).then(|| {
            assert_eq!(options(g), vec![Choice::Target(engine::Target::Player(1))], "an opponent only");
            Choice::Target(engine::Target::Player(1))
        })
    });
    assert_eq!(g.life(1), 18);
}

#[test]
fn cr_701_26a_tap_another_target_creature_and_damage_a_tapped_creature() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Test Starport"])
        .battlefield(1, &["Test Ogre"])
        .hand(0, &["Test Reroute"])
        .at(Step::Main1, 0)
        .start();
    let (port, ogre) = (bf(&g, 0, "Test Starport"), bf(&g, 1, "Test Ogre"));
    // The Reroute needs a tapped creature: none yet.
    assert!(!g.has_option(Choice::Cast(g.card(0, "Test Reroute", Zone::Hand))));
    g.choose(Choice::Ability(port, AbilitySlot::Activated(0)));
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target) || g.stack().len() == 1, |_| None);
    if g.kind() == Some(DecisionKind::Target) {
        assert!(!options(&g).contains(&g.tgt(port)), "another");
        g.choose(g.tgt(ogre));
    }
    settle(&mut g);
    assert!(g.obj(ogre).tapped);
    cast(&mut g, "Test Reroute");
    settle_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(ogre)));
    assert_eq!(g.obj(ogre).damage, 2);
}

#[test]
fn cr_608_2h_damage_equal_to_the_sources_power() {
    let mut g = scen().battlefield(0, &["Test Harvester"]).at(Step::Main1, 0).start();
    let h = bf(&g, 0, "Test Harvester");
    g.choose(Choice::Ability(h, AbilitySlot::Activated(0)));
    settle_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(Choice::Target(engine::Target::Player(1))));
    assert_eq!(g.life(1), 17);
}

#[test]
fn cr_115_10_return_up_to_two_non_spacecraft_creatures() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Test Bear"])
        .battlefield(1, &["Test Ogre", "Test Bulwark"])
        .hand(0, &["Test Freighter"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Test Freighter");
    let (ogre, wall) = (bf(&g, 1, "Test Ogre"), bf(&g, 1, "Test Bulwark"));
    target(&mut g, ogre);
    run(&mut g, |g| g.kind() == Some(DecisionKind::Target), |_| None);
    g.choose(g.tgt(wall));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Ogre", Zone::Hand) + g.count_in(1, "Test Bulwark", Zone::Hand), 2);
}

#[test]
fn cr_110_4_you_may_put_a_creature_card_from_your_hand_onto_the_battlefield() {
    let mut g = scen().battlefield(0, &["Plains"]).hand(0, &["Test Seedship", "Test Ogre", "Plains"]).at(Step::Main1, 0).start();
    cast(&mut g, "Test Seedship");
    run(&mut g, |g| g.kind() == Some(DecisionKind::ChooseHandCard) || g.stack().is_empty() && at_priority(g), |_| None);
    let ogre = g.card(0, "Test Ogre", Zone::Hand);
    assert!(options(&g).contains(&Choice::Pass), "may");
    assert!(!options(&g).contains(&Choice::Card(g.card(0, "Plains", Zone::Hand))), "a creature card");
    g.choose(Choice::Card(ogre));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Test Ogre", Zone::Battlefield), 1);
}

#[test]
fn cr_613_1b_gain_control_of_target_sliver() {
    let mut g = scen().battlefield(0, &["Test Overlord"]).battlefield(1, &["Test Sliver", "Test Bear"]).at(Step::Main1, 0).start();
    let o = bf(&g, 0, "Test Overlord");
    let s = bf(&g, 1, "Test Sliver");
    g.choose(Choice::Ability(o, AbilitySlot::Activated(0)));
    settle(&mut g);
    assert_eq!(g.obj(s).controller, 0, "{}", log(&g));
    next_main(&mut g, 0);
    assert_eq!(g.obj(s).controller, 0, "indefinitely");
}

#[test]
fn cr_101_4_each_player_creates_a_token() {
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Test Rover"])
        .hand(0, &["Cut Propulsion"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Cut Propulsion");
    settle(&mut g);
    assert_eq!(all_bf(&g, 0, "Test Junk").len(), 1);
    assert_eq!(all_bf(&g, 1, "Test Junk").len(), 1);
}

#[test]
fn cr_603_4_upkeep_gain_if_you_control_three_or_more_artifacts() {
    for relics in [2usize, 1] {
        let mut bf0 = vec!["Test Fair"];
        bf0.extend(std::iter::repeat_n("Test Relic", relics));
        let mut g = scen().battlefield(0, &bf0).at(Step::Main1, 1).start();
        next_main(&mut g, 0);
        assert_eq!(g.life(0), if relics == 2 { 21 } else { 20 });
    }
}

#[test]
fn cr_701_8a_destroy_target_creature_or_spacecraft() {
    let mut g = scen().battlefield(0, &["Plains"]).battlefield(1, &["Test Ship", "Test Relic"]).hand(0, &["Test Oblivion"]).at(Step::Main1, 0).start();
    let ship = bf(&g, 1, "Test Ship");
    cast(&mut g, "Test Oblivion");
    settle_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::Target)).then(|| {
            assert!(!options(g).contains(&g.tgt(bf(g, 1, "Test Relic"))));
            g.tgt(ship)
        })
    });
    assert_eq!(g.count_in(1, "Test Ship", Zone::Graveyard), 1);
    let mut g = scen().battlefield(0, &["Plains"]).battlefield(1, &["Test Ogre"]).hand(0, &["Test Vote Out"]).at(Step::Main1, 0).start();
    cast(&mut g, "Test Vote Out");
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Ogre", Zone::Graveyard), 1);
}
