//! Cube cards supported in M7a (threats first): equipment triggers, static
//! locks and taxes, replacement effects, choices of permanents, flicker,
//! coin flips, explore, discard and draw triggers.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::hidden::{key_kind, key_payload, KEY_COIN};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

/// Every decision exposed (`auto_single` off).
fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Pass priority, answer forced decisions, pay with the first sources and
/// order triggers in the first order offered until the stack is empty (or
/// another decision is asked).
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

fn pay(g: &mut Game) {
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(at_priority);
    }
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
}

/// Pass, decline attacks and blocks, answer forced decisions until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ if d.options.len() == 1 => d.options[0].choice,
            k => panic!("quiet_until: unexpected {k:?}: {:?}", g.options_desc()),
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

#[test]
fn skullclamp_whenever_equipped_creature_dies_draw_two() {
    let mut g = scen().battlefield(0, &["Skullclamp", "Swamp", "Test Bear"]).at(Step::Main1, 0).start();
    let (clamp, bear) = (bf(&g, 0, "Skullclamp"), bf(&g, 0, "Test Bear"));
    let hand = g.player(0).hand.len();
    g.choose(act(clamp, 0));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    pay(&mut g);
    settle(&mut g);
    // +1/-1.
    assert_eq!(g.pt(bear), (3, 1));
    let mut g2 = scen()
        .battlefield(0, &["Skullclamp", "Swamp", "Test Hasty"])
        .at(Step::Main1, 0)
        .start();
    let (clamp, x) = (bf(&g2, 0, "Skullclamp"), bf(&g2, 0, "Test Hasty"));
    let before = g2.player(0).hand.len();
    g2.choose(act(clamp, 0));
    g2.choose(g2.tgt(x));
    g2.drive_until(at_priority);
    pay(&mut g2);
    settle(&mut g2);
    // A 2/1 equipped is 3/0 and dies (704.5f); the trigger looks back at
    // the Equipment attached to it (603.10a) and its controller draws two.
    assert_eq!(g2.obj(x).zone, Zone::Graveyard);
    assert_eq!(g2.player(0).hand.len(), before + 2);
    assert_eq!(g2.obj(clamp).attached, None, "704.5n");
    let _ = hand;
}

#[test]
fn thalia_cr_601_2f_noncreature_spells_cost_one_more() {
    let g = scen()
        .battlefield(0, &["Mountain"])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .hand(0, &["Lightning Bolt", "Test Hasty"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let hasty = g.card(0, "Test Hasty", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bolt)), "{{R}} + {{1}} with one Mountain");
    assert!(g.has_option(Choice::Cast(hasty)), "creature spells are not taxed");
    let g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .battlefield(1, &["Thalia, Guardian of Thraben"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bolt)));
}

#[test]
fn clarion_conqueror_cr_602_5_locks_activated_abilities_mana_included() {
    let g = scen()
        .battlefield(0, &["Llanowar Elves", "Mind Stone", "Forest"])
        .battlefield(1, &["Clarion Conqueror"])
        .hand(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    // Elves and Mind Stone can't tap for mana: one Forest can't pay {1}{G}.
    assert!(!g.has_option(Choice::Cast(bear)));
}

#[test]
fn containment_priest_cr_614_1a_exiles_creatures_entering_without_being_cast() {
    // Displacer Kitten flickers a creature: with Containment Priest out it
    // is exiled instead of returning.
    let mut g = scen()
        .battlefield(0, &["Displacer Kitten", "Containment Priest", "Test Bear", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "Test Bear");
    cast(&mut g, 0, "Lightning Bolt");
    // Bolt targets the opponent.
    g.choose(Choice::Target(engine::Target::Player(1)));
    g.drive_until(at_priority);
    pay(&mut g);
    // The cast trigger: Kitten's flicker target.
    settle(&mut g);
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(bear));
        settle(&mut g);
    }
    assert_eq!(g.obj(bear).zone, Zone::Exile, "{}", g.log_lines().join("\n"));
    assert_eq!(g.life(1), 17);
}

#[test]
fn wolverine_damage_heals_other_damage() {
    let mut g = scen()
        .battlefield(0, &["Wolverine, Fierce Fighter"])
        .battlefield(1, &["Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let w = bf(&g, 0, "Wolverine, Fierce Fighter");
    for _ in 0..2 {
        cast(&mut g, 1, "Lightning Bolt");
        g.choose(g.tgt(w));
        g.drive_until(at_priority);
        pay(&mut g);
        settle(&mut g);
    }
    // 3 + 3 to a 3/5: each damage replaces the damage already dealt.
    assert_eq!(g.obj(w).zone, Zone::Battlefield);
    assert_eq!(g.obj(w).damage, 3);
}

#[test]
fn dragons_rage_channeler_cr_508_1d_attacks_each_combat_with_delirium() {
    let mut g = scen()
        .battlefield(0, &["Dragon's Rage Channeler", "Test Bear"])
        .graveyard(0, &["Island", "Lightning Bolt", "Test Bear", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let drc = bf(&g, 0, "Dragon's Rage Channeler");
    assert_eq!(g.pt(drc), (3, 3));
    assert!(g.has_kw(drc, Kw::Flying));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert!(g.obj(drc).attacking, "declared before the choice of the others");
    assert!(!g.has_option(Choice::Card(drc)));
}

#[test]
fn mana_crypt_cr_705_coin_flip_is_a_public_chance_key_and_can_be_forced() {
    let mut lost = 0;
    for seed in 0..20 {
        let mut g = scen().seed(seed).battlefield(0, &["Mana Crypt"]).at(Step::End, 1).start();
        let mut keys = Vec::new();
        for _ in 0..50 {
            if g.turn().active == 0 && g.turn().step == Step::Draw {
                break;
            }
            let d = g.pending_decision().unwrap();
            let c = if d.options.len() == 1 { d.options[0].choice } else { Choice::Pass };
            keys.extend(g.choose(c));
        }
        let coin: Vec<u64> = keys.iter().filter(|&&k| key_kind(k) == KEY_COIN).copied().collect();
        assert_eq!(coin.len(), 1);
        if key_payload(coin[0]) == 0 {
            lost += 1;
            assert_eq!(g.life(0), 17);
        } else {
            assert_eq!(g.life(0), 20);
        }
    }
    assert!(lost > 2 && lost < 18, "{lost}");
}

#[test]
fn faerie_mastermind_triggers_on_an_opponents_second_draw_each_turn() {
    let mut g = scen()
        .battlefield(0, &["Faerie Mastermind"])
        .battlefield(0, &["Island"; 8])
        .at(Step::Main1, 0)
        .start();
    let fm = bf(&g, 0, "Faerie Mastermind");
    let mine = g.player(0).hand.len();
    // {3}{U}: each player draws a card (P1's first this turn: no trigger).
    g.choose(act(fm, 0));
    g.drive_until(at_priority);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), mine + 1);
    // P1's turn: the draw step is their first, the ability their second.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1);
    let mine = g.player(0).hand.len();
    for _ in 0..3 {
        if g.turn().priority == 0 {
            break;
        }
        g.pass();
    }
    assert_eq!(g.turn().priority, 0);
    g.choose(act(fm, 0));
    g.drive_until(at_priority);
    pay(&mut g);
    settle(&mut g);
    // Each player drew one (P0 +1), and P1's second draw made P0 draw (+1).
    assert_eq!(g.player(0).hand.len(), mine + 2, "{}", g.log_lines().join("\n"));
}

#[test]
fn explore_cr_701_44_a_land_goes_to_hand_otherwise_a_counter() {
    let mut g = scen()
        .library(0, &["Lightning Bolt", "Forest"])
        .battlefield(0, &["Sentinel of the Nameless City", "Forest", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, "Sentinel of the Nameless City");
    // Make a Map: attack with the Sentinel.
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(s));
    g.choose(Choice::Pass);
    settle(&mut g);
    let map = bf(&g, 0, "Map");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    g.choose(act(map, 0));
    g.choose(g.tgt(s));
    g.drive_until(at_priority);
    pay(&mut g);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    // A nonland: +1/+1 counter, then "may put it into your graveyard".
    assert_eq!(g.counters_on(s, Counter::P1P1), 1);
    assert_eq!(g.kind(), Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Graveyard), 1);
}

#[test]
fn ivora_whenever_you_discard_a_card() {
    let mut g = scen()
        .battlefield(0, &["Ivora, Insatiable Heir", "Mountain"])
        .hand(0, &["Test Rummage", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let iv = bf(&g, 0, "Ivora, Insatiable Heir");
    cast(&mut g, 0, "Test Rummage");
    // Additional cost: discard a card.
    if g.kind() == Some(DecisionKind::DiscardCost) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.counters_on(iv, Counter::P1P1), 1);
}

#[test]
fn chaos_defiler_chooses_without_targeting() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Mountain", "Mountain"])
        .battlefield(1, &["Test Bear", "Sol Ring", "Forest"])
        .hand(0, &["Chaos Defiler"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Chaos Defiler");
    pay(&mut g);
    settle(&mut g);
    // Nonland permanents of the opponent only.
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent));
    let ring = bf(&g, 1, "Sol Ring");
    assert_eq!(g.pending_decision().unwrap().options.len(), 2);
    g.choose(Choice::Card(ring));
    settle(&mut g);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
}

#[test]
fn frantic_search_untaps_up_to_three_lands_one_set_at_a_time() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Swamp"])
        .hand(0, &["Frantic Search", "Test Bear", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Frantic Search");
    pay(&mut g);
    settle(&mut g);
    // Discard two.
    while g.kind() == Some(DecisionKind::Discard) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent));
    let n = |g: &Game| g.pending_decision().unwrap().options.len();
    let first = n(&g);
    let c = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Card(_))).unwrap();
    g.choose(c);
    assert!(n(&g) < first, "later picks come after the first in slot order");
    g.choose(Choice::Pass);
    let tapped = g.lands_of(0).filter(|&c| g.obj(c).tapped).count();
    assert_eq!(tapped, 2);
}
