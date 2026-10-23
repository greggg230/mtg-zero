//! M7b part 3b (ENGINE.md 27.3): ward (CR 702.21): "Whenever this permanent
//! becomes the target of a spell or ability an opponent controls, counter
//! that spell or ability unless that player pays <cost>" - mana (Raffine,
//! Kappa Cannoneer), life (Sedgemoor Witch, Hexing Squelcher) or a card
//! (Sunset Saboteur); granted ward ("other creatures you control have
//! ward"), and spells that can't be countered (101.2).

mod common;

use common::db;
use engine::az::encode::perm;
use engine::db::{AbilitySlot, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
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

fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress");
}

fn tgt_if(g: &Game, c: CardIdx) -> Option<Choice> {
    (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(c))
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

/// The status floats of permanent `c` in `viewer`'s encoding.
fn perm_status(g: &Game, viewer: PlayerId, c: CardIdx) -> Vec<f32> {
    use engine::az::encode::{self, CODE, NUM_FEATURES, PERM_W};
    let mut x = vec![0.0; NUM_FEATURES];
    g.encode_features(viewer, &mut x);
    let ctl = g.obj(c).controller;
    let s = g.permanents_of(ctl).position(|x| x == c).unwrap();
    let o = if ctl == viewer { encode::O_MY_PERM } else { encode::O_OPP_PERM } + s * PERM_W + CODE;
    x[o..o + PERM_W - CODE].to_vec()
}

/// P1's turn: P1 has `lands` and `hand`; P0 has `mine`.
fn scen(mine: &[&str], lands: &[&str], hand: &[&str]) -> Game {
    Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, mine)
        .battlefield(1, lands)
        .hand(1, hand)
        .at(Step::Main1, 1)
        .start()
}

/// P1 casts Lightning Bolt at `target`; the ward decision is answered by
/// `pay` (for the payer); returns the chance keys.
fn bolt_at(g: &mut Game, target: CardIdx, pay: impl Fn(&Game) -> Option<Choice>) {
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(g, |g| tgt_if(g, target));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::YesNo) | Some(DecisionKind::Discard) => {
            assert_eq!(g.pending_decision().unwrap().player, 1, "the targeting spell's controller pays");
            pay(g)
        }
        _ => None,
    });
}

#[test]
fn ward_life_counters_unless_its_controller_pays_cr_702_21a() {
    // Sedgemoor Witch: "Ward-Pay 3 life."
    let mut g = scen(&["Sedgemoor Witch"], &["Mountain"], &["Lightning Bolt"]);
    let w = bf(&g, 0, "Sedgemoor Witch");
    bolt_at(&mut g, w, |_| Some(Choice::Pass));
    assert_eq!(g.obj(w).zone, Zone::Battlefield, "countered");
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Graveyard), 1);
    assert_eq!(g.life(1), 20);
    let mut g = scen(&["Sedgemoor Witch"], &["Mountain"], &["Lightning Bolt"]);
    let w = bf(&g, 0, "Sedgemoor Witch");
    bolt_at(&mut g, w, |_| Some(Choice::Yes));
    assert_eq!(g.life(1), 17, "paid 3 life");
    assert_eq!(g.obj(w).zone, Zone::Graveyard, "the Bolt resolved");
}

#[test]
fn ward_life_cant_be_paid_without_that_much_life_cr_119_4() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Sedgemoor Witch"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .life(1, 2)
        .at(Step::Main1, 1)
        .start();
    let w = bf(&g, 0, "Sedgemoor Witch");
    bolt_at(&mut g, w, |g| {
        assert_eq!(options(g), vec![Choice::Pass]);
        None
    });
    assert_eq!(g.obj(w).zone, Zone::Battlefield);
}

#[test]
fn ward_mana_is_paid_with_mana_cr_702_21a() {
    // Raffine: "Flying, ward {1}" (1/4).
    let mut g = scen(&["Raffine, Scheming Seer"], &["Mountain", "Island"], &["Lightning Bolt"]);
    let r = bf(&g, 0, "Raffine, Scheming Seer");
    bolt_at(&mut g, r, |_| Some(Choice::Yes));
    assert_eq!(g.obj(r).damage, 3, "paid {{1}}: the Bolt resolved");
    assert!(g.lands_of(1).all(|c| g.obj(c).tapped));
    // Without the mana, only "don't pay".
    let mut g = scen(&["Raffine, Scheming Seer"], &["Mountain"], &["Lightning Bolt"]);
    let r = bf(&g, 0, "Raffine, Scheming Seer");
    bolt_at(&mut g, r, |g| {
        assert_eq!(options(g), vec![Choice::Pass]);
        None
    });
    assert_eq!(g.obj(r).damage, 0);
}

#[test]
fn ward_discard_is_a_card_of_the_payers_choice_cr_702_21b() {
    // Sunset Saboteur: "Ward-Discard a card." A `Discard` decision: a hand
    // card pays, PASS doesn't.
    let mut g = scen(&["Sunset Saboteur"], &["Mountain"], &["Lightning Bolt", "Island", "Forest"]);
    let s = bf(&g, 0, "Sunset Saboteur");
    let forest = g.card(1, "Forest", Zone::Hand);
    bolt_at(&mut g, s, |g| {
        assert!(g.has_option(Choice::Pass));
        Some(Choice::Card(forest))
    });
    assert_eq!(g.obj(forest).zone, Zone::Graveyard);
    assert_eq!(g.obj(s).zone, Zone::Graveyard, "paid: the Bolt resolved");
    // With no other card in hand, the Bolt is countered.
    let mut g = scen(&["Sunset Saboteur"], &["Mountain"], &["Lightning Bolt"]);
    let s = bf(&g, 0, "Sunset Saboteur");
    bolt_at(&mut g, s, |g| {
        assert_eq!(options(g), vec![Choice::Pass]);
        None
    });
    assert_eq!(g.obj(s).zone, Zone::Battlefield);
}

#[test]
fn ward_doesnt_trigger_for_its_controllers_spells() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Sedgemoor Witch", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .start();
    let w = bf(&g, 0, "Sedgemoor Witch");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, w));
    resolve_with(&mut g, |g| panic!("no ward decision: {:?}", g.kind()));
    assert_eq!(g.obj(w).zone, Zone::Graveyard);
}

#[test]
fn ward_counters_an_ability_cr_702_21a() {
    // P1's Walking Ballista pings P0's Sedgemoor Witch: the ping is
    // countered unless P1 pays 3 life.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Sedgemoor Witch"])
        .battlefield(1, &["Walking Ballista"])
        .counters(1, "Walking Ballista", Counter::P1P1, 1)
        .at(Step::Main1, 1)
        .start();
    let w = bf(&g, 0, "Sedgemoor Witch");
    let wb = bf(&g, 1, "Walking Ballista");
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    finish(&mut g, |g| tgt_if(g, w));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Pass));
    assert_eq!(g.obj(w).damage, 0, "the ping was countered");
}

#[test]
fn ward_cant_counter_a_spell_that_cant_be_countered_cr_101_2() {
    // P1 controls Hexing Squelcher: "Spells you control can't be
    // countered." Its Bolt at the ward permanent resolves anyway.
    let mut g = scen(&["Sedgemoor Witch"], &["Mountain", "Hexing Squelcher"], &["Lightning Bolt"]);
    let w = bf(&g, 0, "Sedgemoor Witch");
    bolt_at(&mut g, w, |_| Some(Choice::Pass));
    assert_eq!(g.obj(w).zone, Zone::Graveyard);
}

#[test]
fn granted_ward_protects_the_other_creatures_cr_702_21a() {
    // Hexing Squelcher: "Other creatures you control have 'Ward-Pay 2
    // life.'" (and has ward itself).
    let mut g = scen(&["Hexing Squelcher", "Test Bear"], &["Mountain"], &["Lightning Bolt"]);
    let bear = bf(&g, 0, "Test Bear");
    bolt_at(&mut g, bear, |_| Some(Choice::Pass));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "countered");
    let mut g = scen(&["Hexing Squelcher", "Test Bear"], &["Mountain"], &["Lightning Bolt"]);
    let bear = bf(&g, 0, "Test Bear");
    bolt_at(&mut g, bear, |_| Some(Choice::Yes));
    assert_eq!(g.life(1), 18);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn a_creature_that_lost_its_abilities_has_no_granted_ward_cr_613_7() {
    // P1's Oko +1 makes P0's Test Bear (ward from Hexing Squelcher) an Elk
    // with no abilities: the Oko ability itself triggers ward (P1 pays 2
    // life); afterwards a Bolt at the Elk doesn't.
    let mut g = scen(&["Hexing Squelcher", "Test Bear"], &["Mountain", "Oko, Thief of Crowns"], &["Lightning Bolt"]);
    let bear = bf(&g, 0, "Test Bear");
    let oko = bf(&g, 1, "Oko, Thief of Crowns");
    g.choose(Choice::Ability(oko, AbilitySlot::Activated(1)));
    finish(&mut g, |g| tgt_if(g, bear));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.life(1), 18);
    assert_eq!(g.pt(bear), (3, 3), "an Elk");
    let st = perm_status(&g, 1, bear);
    assert_eq!((st[perm::LOST_ABILITIES], st[perm::WARD]), (1.0, 0.0), "encoded: no abilities, no ward");
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    finish(&mut g, |g| tgt_if(g, bear));
    resolve_with(&mut g, |g| panic!("no ward: {:?}", g.kind()));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
}

#[test]
fn ward_triggers_for_each_instance_cr_702_21a() {
    // Hexing Squelcher has its own ward and is not "another creature": one
    // trigger. A second Squelcher grants it ward too: two triggers.
    let mut g = scen(&["Hexing Squelcher", "Hexing Squelcher"], &["Mountain"], &["Lightning Bolt"]);
    let sq = bf(&g, 0, "Hexing Squelcher");
    let asked = std::cell::Cell::new(0);
    bolt_at(&mut g, sq, |_| {
        asked.set(asked.get() + 1);
        Some(Choice::Yes)
    });
    assert_eq!(asked.get(), 2);
    assert_eq!(g.life(1), 16);
    assert_eq!(g.obj(sq).zone, Zone::Graveyard);
}

#[test]
fn ward_and_its_costs_are_in_the_encoding() {
    // Kappa Cannoneer: ward {4}; Sedgemoor Witch: ward-pay 3 life; Sunset
    // Saboteur: ward-discard a card; Hexing Squelcher: ward-pay 2 life, and
    // the other creatures have it too (two wards each). Public: the same
    // for both viewers.
    let g = scen(&["Kappa Cannoneer", "Sedgemoor Witch", "Sunset Saboteur", "Hexing Squelcher"], &["Mountain"], &[]);
    for viewer in [0, 1] {
        let st = |name: &str| perm_status(&g, viewer, bf(&g, 0, name));
        let k = st("Kappa Cannoneer");
        assert_eq!((k[perm::WARD], k[perm::WARD_MANA], k[perm::WARD_LIFE], k[perm::WARD_DISCARD]), (1.0, 1.0, 0.5, 0.0));
        let w = st("Sedgemoor Witch");
        assert_eq!((w[perm::WARD], w[perm::WARD_MANA], w[perm::WARD_LIFE], w[perm::WARD_DISCARD]), (1.0, 0.0, 0.75, 0.0));
        let s = st("Sunset Saboteur");
        assert_eq!((s[perm::WARD], s[perm::WARD_LIFE], s[perm::WARD_DISCARD]), (1.0, 0.5, 1.0));
        let h = st("Hexing Squelcher");
        assert_eq!((h[perm::WARD], h[perm::WARD_LIFE], h[perm::LOST_ABILITIES]), (0.5, 0.5, 0.0));
    }
}
