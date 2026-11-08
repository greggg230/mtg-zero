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

/// Vanilla test cards (all cost {1}).
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

