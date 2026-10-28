//! M7b planeswalkers (ENGINE.md 24), part 2: Karn, Scion of Urza; Minsc &
//! Boo, Timeless Heroes; Narset, Parter of Veils; Nissa, Who Shakes the
//! World; Oko, Thief of Crowns; Tamiyo, Collector of Tales; Teferi, Hero of
//! Dominaria; Teferi, Time Raveler; Tezzeret, Cruel Captain; The Wandering
//! Emperor; Ugin, Eye of the Storms; Wrenn and Six.

mod common;

use common::db;
use engine::db::{types, AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn scen() -> Scenario {
    manual().library(0, &["Island"; 10]).library(1, &["Forest"; 10])
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

fn loyalty(g: &Game, c: CardIdx) -> u32 {
    g.counters_on(c, Counter::Loyalty)
}

fn tgt_player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

fn settle(g: &mut Game) {
    while let Some(d) = g.pending_decision() {
        if d.kind == DecisionKind::Priority || d.options.len() != 1 {
            return;
        }
        let c = d.options[0].choice;
        g.choose(c);
    }
}

fn pay(g: &mut Game) {
    while !at_priority(g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

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

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    settle(g);
}

fn activate(g: &mut Game, c: CardIdx, i: u8, answer: impl Fn(&Game) -> Option<Choice>) {
    g.choose(act(c, i));
    while !at_priority(g) {
        let ch = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(ch);
    }
}

fn none(_: &Game) -> Option<Choice> {
    None
}

fn target(c: CardIdx) -> impl Fn(&Game) -> Option<Choice> {
    move |g: &Game| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(c))
}

// ---- Karn, Scion of Urza ------------------------------------------------------

const KARN: &str = "Karn, Scion of Urza";

#[test]
fn karn_plus_one_the_opponent_chooses_the_card_the_other_is_exiled_with_silver() {
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Test Giant", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[KARN])
        .at(Step::Main1, 0)
        .start();
    let karn = bf(&g, 0, KARN);
    activate(&mut g, karn, 0, none);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::PutInHand) {
            let d = g.pending_decision().unwrap();
            assert_eq!(d.player, 1, "an opponent chooses");
            let giant = g.find_card(0, "Test Giant", Zone::Library).unwrap();
            return Some(Choice::Card(giant));
        }
        None
    });
    assert_eq!(g.count_in(0, "Test Giant", Zone::Hand), 1);
    let bolt = g.card(0, "Lightning Bolt", Zone::Exile);
    // Exiled with a silver counter on it (a counter on a card in exile).
    let cs = g.counters_of(bolt);
    assert!(matches!(cs[..], [(Counter::Named(_), 1)]), "{cs:?}");
    assert!(g.knows(1, g.card(0, "Test Giant", Zone::Hand)), "revealed");
    // -1: a card you own with a silver counter, from exile into your hand.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    activate(&mut g, karn, 1, none);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseExiled) {
            return Some(Choice::Card(g.card(0, "Lightning Bolt", Zone::Exile)));
        }
        None
    });
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Hand), 1);
}

#[test]
fn karn_minus_two_makes_a_construct_that_counts_artifacts() {
    // ruling: 7e0c0fb5 ae903f55
    let mut g = scen().battlefield(0, &[KARN, "Sol Ring"]).at(Step::Main1, 0).start();
    let karn = bf(&g, 0, KARN);
    activate(&mut g, karn, 2, none);
    resolve_with(&mut g, none);
    let c = bf(&g, 0, "Construct");
    assert!(g.is_type(c, types::ARTIFACT) && g.is_creature(c));
    assert_eq!(g.pt(c), (2, 2), "itself and Sol Ring");
    assert_eq!(loyalty(&g, karn), 3);
}

// ---- Minsc & Boo, Timeless Heroes ----------------------------------------------
//
// "When Minsc & Boo enters and at the beginning of your upkeep, you may
// create Boo, a legendary 1/1 red Hamster creature token with trample and
// haste." Boo is red in the IR since mtg-bot 8ea9fcca (docs/ir_defects.md
// 12; it was tested on a corrected copy of the IR before).

const MINSC: &str = "Minsc & Boo, Timeless Heroes";

fn minsc_scen() -> Scenario {
    scen()
}

#[test]
fn minsc_and_boo_make_boo_and_fling_a_hamster() {
    let mut g = minsc_scen()
        .battlefield(0, &["Mountain", "Forest", "Mountain", "Forest"])
        .hand(0, &[MINSC])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, MINSC);
    pay(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    let m = bf(&g, 0, MINSC);
    let boo = bf(&g, 0, "Boo");
    assert!(g.has_kw(boo, Kw::Trample) && g.has_kw(boo, Kw::Haste));
    assert_eq!(g.colors_of(boo), 1 << 3, "red");
    assert!(g.def_of(boo).supertypes & engine::db::supertypes::LEGENDARY != 0);
    // +1: three +1/+1 counters on up to one target creature with trample
    // or haste.
    let bear = bf(&g, 1, "Test Bear");
    g.choose(act(m, 0));
    assert!(!g.has_option(g.tgt(bear)), "no trample or haste");
    g.choose(g.tgt(boo));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.pt(boo), (4, 4));
    // Next turn: the upkeep trigger may make another Boo (legend rule).
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.count_in(0, "Boo", Zone::Battlefield), 1);
    // -2: sacrifice Boo (a Hamster with power 4): 4 damage to any target and
    // draw 4.
    let hand = g.player(0).hand.len();
    activate(&mut g, m, 1, none);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Sacrifice) => g.find_card(0, "Boo", Zone::Battlefield).map(Choice::Card),
        Some(DecisionKind::Target) => Some(tgt_player(1)),
        _ => None,
    });
    assert_eq!(g.life(1), 16);
    assert_eq!(g.player(0).hand.len(), hand + 4);
}

#[test]
fn minsc_and_boo_sacrificing_a_non_hamster_draws_nothing() {
    let mut g = minsc_scen().battlefield(0, &[MINSC, "Test Giant"]).at(Step::Main1, 0).start();
    let m = bf(&g, 0, MINSC);
    let hand = g.player(0).hand.len();
    activate(&mut g, m, 1, none);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(tgt_player(1)),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Test Giant", Zone::Graveyard), 1);
    assert_eq!(g.life(1), 15);
    assert_eq!(g.player(0).hand.len(), hand);
}

// ---- Narset, Parter of Veils -----------------------------------------------------

#[test]
fn narset_each_opponent_draws_at_most_one_card_each_turn() {
    // ruling: 1a963653
    let mut g = scen()
        .battlefield(0, &["Narset, Parter of Veils"])
        .battlefield(1, &["Island"])
        .hand(1, &["Ancestral Recall"])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Ancestral Recall");
    g.choose(tgt_player(1));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.player(1).hand.len(), 1, "one of three");
    // Its controller isn't limited.
    let mut g = scen()
        .battlefield(0, &["Narset, Parter of Veils", "Island"])
        .hand(0, &["Ancestral Recall"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Ancestral Recall");
    g.choose(tgt_player(0));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.player(0).hand.len(), 3);
}

#[test]
fn narset_minus_two_finds_a_noncreature_nonland_card() {
    let mut g = manual()
        .library(0, &["Test Bear", "Island", "Lightning Bolt", "Consider", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Narset, Parter of Veils"])
        .at(Step::Main1, 0)
        .start();
    let n = bf(&g, 0, "Narset, Parter of Veils");
    activate(&mut g, n, 0, none);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::PutInHand) {
            // Only the Bolt and Consider (not the Bear, not the Island).
            let names: Vec<String> = g
                .pending_decision()
                .unwrap()
                .options
                .iter()
                .filter_map(|o| match o.choice {
                    Choice::Card(c) => Some(g.name(c).to_string()),
                    _ => None,
                })
                .collect();
            assert_eq!(names.len(), 2, "{names:?}");
            return g.find_card(0, "Lightning Bolt", Zone::Library).map(Choice::Card);
        }
        None
    });
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Hand), 1);
    assert_eq!(g.player(0).library.len(), 4);
    assert_eq!(g.player(0).library.last().map(|&c| g.name(c)), Some("Island"), "the rest on the bottom");
}

// ---- Nissa, Who Shakes the World -------------------------------------------------

const NISSA: &str = "Nissa, Who Shakes the World";

#[test]
fn nissa_forests_tap_for_an_additional_green() {
    let mut g = scen().battlefield(0, &[NISSA, "Forest"]).hand(0, &["Test Bear"]).at(Step::Main1, 0).start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bear)), "{{1}}{{G}} from one Forest (CR 605.1b)");
    cast(&mut g, 0, "Test Bear");
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1);
}

#[test]
fn nissa_animates_a_land_and_ultimates() {
    // ruling: 743f8a7f
    let mut g = scen().battlefield(0, &[NISSA, "Forest", "Island"]).at(Step::Main1, 0).start();
    let forest = bf(&g, 0, "Forest");
    let n = bf(&g, 0, NISSA);
    g.choose(act(n, 0));
    g.choose(g.tgt(forest));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert!(g.is_creature(forest) && g.is_type(forest, types::LAND));
    assert_eq!(g.pt(forest), (3, 3));
    assert!(g.has_kw(forest, Kw::Haste) && g.has_kw(forest, Kw::Vigilance));
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.is_creature(forest), "no duration");
    // -8: the emblem, and any number of Forests onto the battlefield.
    let mut g = manual()
        .library(0, &["Forest", "Island", "Forest", "Forest"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[NISSA, "Island"])
        .counters(0, NISSA, Counter::Loyalty, 8)
        .at(Step::Main1, 0)
        .start();
    let n = bf(&g, 0, NISSA);
    activate(&mut g, n, 1, none);
    let mut picks = 0;
    for _ in 0..50 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        if g.kind() == Some(DecisionKind::SearchLibrary) && picks < 2 {
            let c = g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice;
            g.choose(c);
            picks += 1;
            continue;
        }
        if g.kind() == Some(DecisionKind::SearchLibrary) {
            g.choose(Choice::Pass);
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.count_in(0, "Forest", Zone::Battlefield), 2);
    assert!(g.battlefield().iter().filter(|&&c| g.name(c) == "Forest").all(|&c| g.obj(c).tapped));
    assert_eq!(g.player(0).library.len(), 2);
    let island = bf(&g, 0, "Island");
    assert!(g.has_kw(island, Kw::Indestructible), "the emblem: lands you control");
}

// ---- Oko, Thief of Crowns ----------------------------------------------------------

const OKO: &str = "Oko, Thief of Crowns";

#[test]
fn oko_food_elk_and_exchange() {
    let mut g = scen().battlefield(0, &[OKO]).battlefield(1, &["Test Giant", "Test Bear"]).at(Step::Main1, 0).start();
    let oko = bf(&g, 0, OKO);
    activate(&mut g, oko, 0, none);
    resolve_with(&mut g, none);
    let food = bf(&g, 0, "Food");
    assert!(g.is_type(food, types::ARTIFACT));
    assert_eq!(loyalty(&g, oko), 6);
    // Next turn: +1 on the Giant: a 3/3 green Elk with no abilities.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    let giant = bf(&g, 1, "Test Giant");
    activate(&mut g, oko, 1, target(giant));
    resolve_with(&mut g, none);
    assert_eq!(g.pt(giant), (3, 3));
    assert!(!g.has_kw(giant, Kw::Trample) && !g.has_kw(giant, Kw::Deathtouch));
    assert_eq!(g.colors_of(giant), 1 << 4);
    let elk = g.db().subtype_id("Elk").unwrap();
    assert!(g.has_subtype(giant, elk));
    // Next turn: -5: exchange the Food and the Bear (power 3 or less).
    quiet_until(&mut g, |g| g.turn().number == 7 && g.turn().step == Step::Main1 && at_priority(g));
    let bear = bf(&g, 1, "Test Bear");
    g.choose(act(oko, 2));
    g.choose(g.tgt(food));
    assert!(!g.has_option(g.tgt(giant)) || g.power(giant) <= 3);
    g.choose(g.tgt(bear));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.obj(bear).controller, 0);
    assert_eq!(g.obj(food).controller, 1);
}

#[test]
fn oko_an_elked_food_is_no_artifact_and_has_no_ability() {
    // CR 205.1a: "becomes a green Elk creature": its card types become
    // creature; it loses all abilities (613.1f).
    let mut g = scen().battlefield(0, &[OKO]).at(Step::Main1, 0).start();
    let oko = bf(&g, 0, OKO);
    activate(&mut g, oko, 0, none);
    resolve_with(&mut g, none);
    let food = bf(&g, 0, "Food");
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    activate(&mut g, oko, 1, target(food));
    resolve_with(&mut g, none);
    assert!(g.is_creature(food) && !g.is_type(food, types::ARTIFACT));
    let food_sub = g.db().subtype_id("Food").unwrap();
    assert!(!g.has_subtype(food, food_sub));
    assert!(!g.has_option(act(food, 0)), "its sacrifice ability is gone");
}

// ---- Tamiyo, Collector of Tales ----------------------------------------------------

const TAMIYO: &str = "Tamiyo, Collector of Tales";

#[test]
fn tamiyo_opponents_cant_make_you_discard_or_sacrifice() {
    let mut g = scen()
        .battlefield(0, &[TAMIYO, "Test Bear"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Swamp", "Swamp", "Swamp"])
        .hand(1, &["Thoughtseize", "Sheoldred's Edict"])
        .at(Step::Main1, 1)
        .start();
    cast(&mut g, 1, "Thoughtseize");
    g.choose(tgt_player(0));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Hand), 1, "no discard");
    cast(&mut g, 1, "Sheoldred's Edict");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(0)));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1, "no sacrifice");
}

#[test]
fn tamiyo_plus_one_names_a_card_and_minus_three_returns_a_card() {
    let mut g = manual()
        .library(0, &["Lightning Bolt", "Island", "Lightning Bolt", "Consider", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[TAMIYO])
        .at(Step::Main1, 0)
        .start();
    let t = bf(&g, 0, TAMIYO);
    activate(&mut g, t, 0, none);
    let bolt_def = g.db().id("Lightning Bolt").unwrap();
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::NameCard) {
            let d = g.pending_decision().unwrap();
            // Nonland names in the library, and a name not in it.
            assert!(d.options.iter().any(|o| o.choice == Choice::Pass));
            assert!(!d.options.iter().any(|o| matches!(o.choice, Choice::Def(x) if g.db().def(x).is(types::LAND))));
            return Some(Choice::Def(bolt_def));
        }
        None
    });
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Hand), 2);
    assert_eq!(g.player(0).graveyard.len(), 2, "the Island and Consider");
    assert_eq!(g.player(0).library.len(), 1);
    // -3: return target card from your graveyard to your hand.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    let consider = g.card(0, "Consider", Zone::Graveyard);
    activate(&mut g, t, 1, target(consider));
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(0, "Consider", Zone::Hand), 1);
}

// ---- Teferi, Hero of Dominaria -----------------------------------------------------

const TEFERI_H: &str = "Teferi, Hero of Dominaria";

#[test]
fn teferi_hero_draws_and_untaps_two_lands_at_the_end_step() {
    let mut g = scen()
        .battlefield(0, &[TEFERI_H, "Mountain", "Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let t = bf(&g, 0, TEFERI_H);
    for _ in 0..2 {
        cast(&mut g, 0, "Lightning Bolt");
        g.choose(tgt_player(1));
        pay(&mut g);
        resolve_with(&mut g, none);
    }
    activate(&mut g, t, 0, none);
    resolve_with(&mut g, none);
    assert_eq!(g.player(0).hand.len(), 1);
    quiet_until(&mut g, |g| g.turn().step == Step::End && g.kind() == Some(DecisionKind::ChoosePermanent));
    // "Untap up to two lands": two picks (not `PASS`).
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::ChoosePermanent))
            .then(|| g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).map(|o| o.choice))
            .flatten()
    });
    let untapped = g.lands_of(0).filter(|&c| !g.obj(c).tapped).count();
    assert_eq!(untapped, 3, "two untapped at the end step, one never tapped");
}

#[test]
fn teferi_hero_tucks_a_permanent_third_from_the_top_and_his_emblem_exiles() {
    let mut g = scen().battlefield(0, &[TEFERI_H]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let t = bf(&g, 0, TEFERI_H);
    let bear = bf(&g, 1, "Test Bear");
    activate(&mut g, t, 1, target(bear));
    resolve_with(&mut g, none);
    let lib = &g.player(1).library;
    assert_eq!(g.name(lib[lib.len() - 3]), "Test Bear");
    // The emblem: "whenever you draw a card, exile target permanent an
    // opponent controls".
    let mut g = scen().emblem(0, TEFERI_H).battlefield(1, &["Test Bear"]).at(Step::Upkeep, 0).start();
    let bear = bf(&g, 1, "Test Bear");
    resolve_with(&mut g, target(bear));
    quiet_until(&mut g, |g| g.turn().step == Step::Main1);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
}

// ---- Teferi, Time Raveler -------------------------------------------------------------

const TEFERI_T: &str = "Teferi, Time Raveler";

#[test]
fn teferi_time_raveler_opponents_cast_at_sorcery_speed_only() {
    let mut g = scen()
        .battlefield(0, &[TEFERI_T])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.pass();
    assert_eq!(g.pending_decision().unwrap().player, 1);
    assert!(!g.has_option(Choice::Cast(bolt)), "not during P0's turn");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(Choice::Cast(bolt)), "in P1's own main phase");
}

#[test]
fn teferi_time_raveler_lets_sorceries_flash_until_your_next_turn_and_bounces() {
    let mut g = scen()
        .battlefield(0, &[TEFERI_T, "Island", "Island"])
        .hand(0, &["Ponder"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let t = bf(&g, 0, TEFERI_T);
    let ponder = g.card(0, "Ponder", Zone::Hand);
    activate(&mut g, t, 0, none);
    resolve_with(&mut g, none);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Upkeep && at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(g.has_option(Choice::Cast(ponder)), "a sorcery during P1's upkeep");
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Upkeep && at_priority(g));
    assert!(!g.has_option(Choice::Cast(ponder)), "the effect ended");
    // -3: return up to one target artifact, creature or enchantment; draw.
    quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    let bear = bf(&g, 1, "Test Bear");
    let hand = g.player(0).hand.len();
    activate(&mut g, t, 1, target(bear));
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Hand), 1);
    assert_eq!(g.player(0).hand.len(), hand + 1);
}

// ---- Tezzeret, Cruel Captain ----------------------------------------------------------

const TEZZ: &str = "Tezzeret, Cruel Captain";

#[test]
fn tezzeret_gains_loyalty_as_artifacts_enter_and_untaps() {
    let mut g = scen().battlefield(0, &[TEZZ, "Island"]).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let tz = bf(&g, 0, TEZZ);
    cast(&mut g, 0, "Sol Ring");
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(loyalty(&g, tz), 5, "an artifact entered");
    // 0: untap target artifact or creature.
    let ring = bf(&g, 0, "Sol Ring");
    let island = bf(&g, 0, "Island");
    assert!(g.obj(island).tapped);
    activate(&mut g, tz, 0, target(ring));
    resolve_with(&mut g, none);
    assert_eq!(g.counters_on(ring, Counter::P1P1), 0, "not an artifact creature");
}

#[test]
fn tezzeret_minus_three_finds_a_cheap_artifact_and_his_emblem_animates() {
    let mut g = manual()
        .library(0, &["Island", "Sol Ring", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[TEZZ])
        .at(Step::Main1, 0)
        .start();
    let tz = bf(&g, 0, TEZZ);
    activate(&mut g, tz, 1, none);
    resolve_with(&mut g, |g| {
        (g.kind() == Some(DecisionKind::SearchLibrary))
            .then(|| g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice)
    });
    assert_eq!(g.count_in(0, "Sol Ring", Zone::Hand), 1);
    // The emblem: at the beginning of combat on your turn, three +1/+1
    // counters on target artifact you control; a noncreature one becomes a
    // 0/0 Robot artifact creature.
    let mut g = scen().emblem(0, TEZZ).battlefield(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let ring = bf(&g, 0, "Sol Ring");
    g.pass();
    g.pass();
    resolve_with(&mut g, target(ring));
    assert!(g.is_creature(ring) && g.is_type(ring, types::ARTIFACT));
    assert_eq!(g.pt(ring), (3, 3));
    let robot = g.db().subtype_id("Robot").unwrap();
    assert!(g.has_subtype(ring, robot));
}

// ---- The Wandering Emperor ---------------------------------------------------------------

const EMPEROR: &str = "The Wandering Emperor";

#[test]
fn the_wandering_emperor_counters_first_strike_and_samurai() {
    let mut g = scen().battlefield(0, &[EMPEROR, "Test Bear"]).at(Step::Main1, 0).start();
    let e = bf(&g, 0, EMPEROR);
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(e, 0));
    assert!(g.has_option(Choice::Pass), "up to one target creature");
    g.choose(g.tgt(bear));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.pt(bear), (3, 3));
    assert!(g.has_kw(bear, Kw::FirstStrike));
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(!g.has_kw(bear, Kw::FirstStrike), "until end of turn");
    activate(&mut g, e, 1, none);
    resolve_with(&mut g, none);
    let sam = bf(&g, 0, "Samurai");
    assert_eq!(g.pt(sam), (2, 2));
    assert!(g.has_kw(sam, Kw::Vigilance));
}

// ---- Ugin, Eye of the Storms --------------------------------------------------------------

const UGIN: &str = "Ugin, Eye of the Storms";

#[test]
fn ugin_cast_triggers_exile_colored_permanents() {
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Sol Ring", "Wasteland", "Wasteland", "Wasteland"])
        .hand(0, &[UGIN, "Mind Stone"])
        .battlefield(1, &["Test Bear", "Mox Jet"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let jet = bf(&g, 1, "Mox Jet");
    cast(&mut g, 0, UGIN);
    while !at_priority(&g) && g.kind() != Some(DecisionKind::Target) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    // "When you cast this spell, exile up to one target permanent that's
    // one or more colors."
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    // One trigger: the spell's "whenever you cast a colorless spell" works
    // only on the battlefield (CR 113.6).
    assert_eq!(g.stack().len(), 2);
    assert!(g.waiting_triggers().is_empty());
    assert!(!g.has_option(g.tgt(jet)), "colorless");
    g.choose(g.tgt(bear));
    resolve_with(&mut g, none);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    let u = bf(&g, 0, UGIN);
    assert_eq!(loyalty(&g, u), 7);
}

#[test]
fn ugin_gains_draws_makes_mana_and_ultimates() {
    let mut g = scen().battlefield(0, &[UGIN]).at(Step::Main1, 0).start();
    let u = bf(&g, 0, UGIN);
    activate(&mut g, u, 0, none);
    resolve_with(&mut g, none);
    assert_eq!(g.life(0), 23);
    assert_eq!(g.player(0).hand.len(), 1);
    assert_eq!(loyalty(&g, u), 9);
    let mut g = scen().battlefield(0, &[UGIN]).at(Step::Main1, 0).start();
    let u = bf(&g, 0, UGIN);
    activate(&mut g, u, 1, none);
    resolve_with(&mut g, none);
    assert_eq!(g.player(0).pool.amt[5], 3);
    // -11: colorless nonland cards exiled, cast for free this turn.
    let mut g = manual()
        .library(0, &["Sol Ring", "Island", "Mind Stone", "Lightning Bolt"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &[UGIN])
        .counters(0, UGIN, Counter::Loyalty, 11)
        .at(Step::Main1, 0)
        .start();
    let u = bf(&g, 0, UGIN);
    activate(&mut g, u, 2, none);
    let mut picked = 0;
    for _ in 0..50 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        if g.kind() == Some(DecisionKind::SearchLibrary) {
            let names: Vec<String> = g
                .pending_decision()
                .unwrap()
                .options
                .iter()
                .filter_map(|o| match o.choice {
                    Choice::Def(d) => Some(g.db().def(d).name.clone()),
                    _ => None,
                })
                .collect();
            assert!(names.iter().all(|n| n == "Sol Ring" || n == "Mind Stone"), "{names:?}");
            let c = if picked < 2 { g.pending_decision().unwrap().options.iter().find(|o| o.choice != Choice::Pass).unwrap().choice } else { Choice::Pass };
            picked += 1;
            g.choose(c);
            continue;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    assert_eq!(g.count_in(0, "Sol Ring", Zone::Exile), 1);
    assert_eq!(g.count_in(0, "Mind Stone", Zone::Exile), 1);
    let ring = g.card(0, "Sol Ring", Zone::Exile);
    assert!(g.has_option(Choice::Cast(ring)), "free, this turn");
    g.choose(Choice::Cast(ring));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(0, "Sol Ring", Zone::Battlefield), 1);
}

// ---- Wrenn and Six ----------------------------------------------------------------------------

const W6: &str = "Wrenn and Six";

#[test]
fn wrenn_and_six_regrows_a_land_pings_and_gives_retrace() {
    let mut g = scen().battlefield(0, &[W6]).graveyard(0, &["Wasteland"]).at(Step::Main1, 0).start();
    let w = bf(&g, 0, W6);
    let waste = g.card(0, "Wasteland", Zone::Graveyard);
    g.choose(act(w, 0));
    assert!(g.has_option(Choice::Pass), "up to one target");
    g.choose(g.tgt(waste));
    pay(&mut g);
    resolve_with(&mut g, none);
    assert_eq!(g.count_in(0, "Wasteland", Zone::Hand), 1);
    // -1: 1 damage to any target, a planeswalker included.
    let mut g = scen().battlefield(0, &[W6]).battlefield(1, &["Jace, the Mind Sculptor"]).at(Step::Main1, 0).start();
    let w = bf(&g, 0, W6);
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    activate(&mut g, w, 1, target(jace));
    resolve_with(&mut g, none);
    assert_eq!(loyalty(&g, jace), 2);
    // The emblem: instants and sorceries in your graveyard have retrace
    // (CR 702.81a: cast by discarding a land card in addition).
    let mut g = scen()
        .emblem(0, W6)
        .battlefield(0, &["Mountain"])
        .graveyard(0, &["Lightning Bolt"])
        .hand(0, &["Island", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(bolt)));
    g.choose(Choice::Cast(bolt));
    settle(&mut g);
    g.choose(tgt_player(1));
    while !at_priority(&g) {
        if g.kind() == Some(DecisionKind::DiscardCost) {
            // Only the land card.
            assert_eq!(g.pending_decision().unwrap().options.len(), 1);
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    resolve_with(&mut g, none);
    assert_eq!(g.life(1), 17);
    assert_eq!(g.count_in(0, "Island", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1, "back in the graveyard");
    // Without a land card in hand: no retrace.
    let g = scen().emblem(0, W6).battlefield(0, &["Mountain"]).graveyard(0, &["Lightning Bolt"]).hand(0, &["Test Bear"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(Choice::Cast(g.card(0, "Lightning Bolt", Zone::Graveyard))));
}

#[test]
fn oko_an_elked_thalia_taxes_nothing() {
    // Her tax is a static ability: gone with all her abilities (CR 613.1f).
    let mut g = scen()
        .battlefield(0, &[OKO, "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bolt)), "{{R}} + {{1}}");
    let oko = bf(&g, 0, OKO);
    let thalia = bf(&g, 1, "Thalia, Guardian of Thraben");
    activate(&mut g, oko, 1, target(thalia));
    resolve_with(&mut g, none);
    assert!(!g.has_kw(thalia, Kw::FirstStrike));
    assert!(g.has_option(Choice::Cast(bolt)));
}
