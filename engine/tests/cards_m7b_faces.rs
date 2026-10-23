//! M7b part 2 (ENGINE.md 25): cards with more than one face, part 1: the
//! transforming double-faced cards (CR 701.27, 712): Ulvenwald Oddity,
//! Concealing Curtains, Cecil, Jace, Vryn's Prodigy, Tamiyo, Inquisitive
//! Student, Etali, Primal Conqueror, Outland Liberator (day and night) and
//! Jennifer Walters' transform; each face against its Oracle text.

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

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

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

fn loyalty(g: &Game, c: CardIdx) -> u16 {
    g.counters_on(c, Counter::Loyalty)
}

/// Pay with the first source offered (and the first production) until the
/// payment is done.
fn pay(g: &mut Game) {
    g.drive_until(|g| {
        at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
    });
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana) | Some(DecisionKind::PayPhyrexian)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| {
            at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana))
        });
    }
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(g);
}

/// Run the stack down; `answer` answers what is asked (None: first option).
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

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
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

/// Attack with `attackers` (P0's declare attackers step must be pending),
/// no blocks, answer the rest with `answer`, until P0's second main phase.
fn attack(g: &mut Game, attackers: &[CardIdx], answer: impl Fn(&Game) -> Option<Choice>) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let mut sorted = attackers.to_vec();
    sorted.sort_by_key(|&c| g.battlefield().iter().position(|&x| x == c));
    for &a in &sorted {
        g.choose(Choice::Card(a));
    }
    g.choose(Choice::Pass);
    for _ in 0..500 {
        if g.is_over() || (g.turn().step == Step::Main2 && at_priority(g) && g.stack().is_empty()) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => answer(g).unwrap_or(d.options[0].choice),
        };
        g.choose(c);
    }
    panic!("attack: no progress");
}

// ---- Ulvenwald Oddity // Ulvenwald Behemoth ----------------------------------

const ODDITY: &str = "Ulvenwald Oddity // Ulvenwald Behemoth";

#[test]
fn ulvenwald_oddity_transforms_into_the_behemoth_cr_701_27() {
    // "Trample, haste. {5}{G}{G}: Transform this creature." // "Trample,
    // haste. Other creatures you control get +1/+1 and have trample and
    // haste."
    let mut g = scen().battlefield(0, &[ODDITY, "Test Bear"]).battlefield(0, &["Forest"; 7]).start();
    let o = bf(&g, 0, ODDITY);
    assert_eq!(g.pt(o), (4, 4));
    assert!(g.has_kw(o, Kw::Trample) && g.has_kw(o, Kw::Haste));
    let r = g.obj_ref(o);
    g.choose(act(o, 0));
    pay(&mut g);
    settle(&mut g);
    // The same object (CR 712.18), now Ulvenwald Behemoth.
    assert_eq!(g.obj_ref(o), r);
    assert_eq!(g.name(o), "Ulvenwald Behemoth");
    assert_eq!(g.pt(o), (8, 8));
    assert!(g.has_kw(o, Kw::Trample) && g.has_kw(o, Kw::Haste));
    let bear = bf(&g, 0, "Test Bear");
    assert_eq!(g.pt(bear), (3, 3));
    assert!(g.has_kw(bear, Kw::Trample) && g.has_kw(bear, Kw::Haste));
    // CR 712.8e: its mana value is the front face's; its colour (green)
    // comes from its colour indicator; no mana cost of its own.
    assert_eq!(g.def_of(o).mana_value(), 4);
    assert!(g.def_of(o).cost.is_zero());
    assert_eq!(g.colors_of(o), 1 << 4);
    // The Behemoth has no transform ability.
    assert!(!g.has_option(act(o, 0)));
}

#[test]
fn ulvenwald_oddity_can_be_activated_at_instant_speed() {
    // No "activate only as a sorcery": in the opponent's turn too.
    let mut g = scen().battlefield(0, &[ODDITY]).battlefield(0, &["Forest"; 7]).at(Step::Upkeep, 1).start();
    g.pass(); // P1 passes; P0 has priority
    let o = bf(&g, 0, ODDITY);
    assert!(g.has_option(act(o, 0)));
}

// ---- Concealing Curtains // Revealing Eye ------------------------------------

const CURTAINS: &str = "Concealing Curtains // Revealing Eye";

#[test]
fn concealing_curtains_transforms_at_sorcery_speed_and_the_eye_strips_a_card() {
    // "Defender. {2}{B}: Transform this creature. Activate only as a
    // sorcery." // "Menace. When this creature transforms into Revealing
    // Eye, target opponent reveals their hand. You may choose a nonland card
    // from it. If you do, that player discards that card, then draws a
    // card."
    let mut g = scen()
        .battlefield(0, &[CURTAINS])
        .battlefield(0, &["Swamp"; 3])
        .hand(1, &["Lightning Bolt", "Forest"])
        .start();
    let c = bf(&g, 0, CURTAINS);
    assert_eq!(g.pt(c), (0, 4));
    assert!(g.has_kw(c, Kw::Defender));
    g.choose(act(c, 0));
    pay(&mut g);
    // The trigger (CR 701.27e: it transformed into Revealing Eye).
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.stack().len() > 1);
    assert_eq!(g.name(c), "Revealing Eye");
    assert_eq!(g.pt(c), (3, 4));
    assert!(g.has_kw(c, Kw::Menace) && !g.has_kw(c, Kw::Defender));
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseDiscard) => {
            // A nonland card: the Bolt, not the Forest.
            assert_eq!(g.pending_decision().unwrap().options.iter().filter(|o| o.choice != Choice::Pass).count(), 1);
            Some(Choice::Card(bolt))
        }
        _ => None,
    });
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    // "then draws a card": the Forest and the drawn card.
    assert_eq!(g.player(1).hand.len(), 2);
}

#[test]
fn revealing_eye_draws_nothing_when_no_card_is_chosen() {
    let mut g = scen()
        .battlefield(0, &[CURTAINS])
        .battlefield(0, &["Swamp"; 3])
        .hand(1, &["Lightning Bolt"])
        .start();
    let c = bf(&g, 0, CURTAINS);
    g.choose(act(c, 0));
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseDiscard) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.player(1).hand.len(), 1);
    assert_eq!(g.player(1).library.len(), 10);
}

#[test]
fn concealing_curtains_transform_is_sorcery_speed() {
    let mut g = scen().battlefield(0, &[CURTAINS]).battlefield(0, &["Swamp"; 3]).at(Step::Upkeep, 1).start();
    g.pass();
    let c = bf(&g, 0, CURTAINS);
    assert!(!g.has_option(act(c, 0)));
}

// ---- Cecil, Dark Knight // Cecil, Redeemed Paladin ---------------------------

const CECIL: &str = "Cecil, Dark Knight // Cecil, Redeemed Paladin";

#[test]
fn cecil_loses_life_for_its_damage_and_transforms_at_half_life() {
    // "Deathtouch. Darkness - Whenever Cecil deals damage, you lose that
    // much life. Then if your life total is less than or equal to half your
    // starting life total, untap Cecil and transform it."
    let mut g = scen().battlefield(0, &[CECIL]).life(0, 12).at(Step::Main1, 0).start();
    let c = bf(&g, 0, CECIL);
    assert_eq!(g.pt(c), (2, 3));
    assert!(g.has_kw(c, Kw::Deathtouch));
    attack(&mut g, &[c], |_| None);
    assert_eq!(g.life(1), 18);
    assert_eq!(g.life(0), 10);
    // At 10 = half of 20: untapped, transformed.
    assert_eq!(g.name(c), "Cecil, Redeemed Paladin");
    assert!(!g.obj(c).tapped);
    assert_eq!(g.pt(c), (4, 4));
    assert!(g.has_kw(c, Kw::Lifelink) && !g.has_kw(c, Kw::Deathtouch));
    assert_eq!(g.colors_of(c), 1, "white (its colour indicator)");
}

#[test]
fn cecil_above_half_life_doesnt_transform() {
    let mut g = scen().battlefield(0, &[CECIL]).at(Step::Main1, 0).start();
    let c = bf(&g, 0, CECIL);
    attack(&mut g, &[c], |_| None);
    assert_eq!(g.life(0), 18);
    assert_eq!(g.name(c), CECIL);
    assert!(g.obj(c).tapped);
}

#[test]
fn cecil_dealing_combat_damage_to_two_blockers_triggers_once() {
    // "Whenever Cecil deals damage": combat damage is dealt at once (CR
    // 510.2), one event with the total.
    let mut g = scen().battlefield(0, &[CECIL]).battlefield(1, &["Test Bear", "Test Reacher"]).at(Step::Main1, 0).start();
    let c = bf(&g, 0, CECIL);
    let (b1, b2) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Reacher"));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(c));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    g.choose(Choice::Card(c));
    // Assign 1 to each blocker (deathtouch: 1 is lethal).
    g.drive_until(|g| g.kind() == Some(DecisionKind::AssignDamage));
    g.choose(Choice::Number(1));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(0), 18, "one trigger, that much: 2");
    assert_eq!(g.obj(b1).zone, Zone::Graveyard);
    assert_eq!(g.obj(b2).zone, Zone::Graveyard);
}

#[test]
fn cecil_redeemed_paladin_makes_other_attackers_indestructible() {
    // "Lifelink. Protect - Whenever Cecil attacks, other attacking creatures
    // gain indestructible until end of turn."
    let mut g = scen().battlefield(0, &[CECIL, "Test Bear"]).transformed(0, CECIL).at(Step::Main1, 0).start();
    let c = bf(&g, 0, "Cecil, Redeemed Paladin");
    let bear = bf(&g, 0, "Test Bear");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(c));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    assert!(g.has_kw(bear, Kw::Indestructible));
    assert!(!g.has_kw(c, Kw::Indestructible), "other attacking creatures");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(0), 24, "lifelink: 4");
    assert_eq!(g.life(1), 14);
}

// ---- Jace, Vryn's Prodigy // Jace, Telepath Unbound --------------------------

const JACE: &str = "Jace, Vryn's Prodigy // Jace, Telepath Unbound";

#[test]
fn jace_loots_and_flips_with_five_cards_in_the_graveyard_cr_712_14a() {
    // "{T}: Draw a card, then discard a card. If there are five or more
    // cards in your graveyard, exile Jace, then return him to the
    // battlefield transformed under his owner's control."
    let mut g = scen()
        .battlefield(0, &[JACE])
        .graveyard(0, &["Island"; 4])
        .hand(0, &["Forest"])
        .start();
    let j = bf(&g, 0, JACE);
    assert_eq!(g.pt(j), (0, 2));
    g.choose(act(j, 0));
    let forest = g.card(0, "Forest", Zone::Hand);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Discard) => g.pending_decision().unwrap().options.iter().find(|o| o.choice == Choice::Card(forest)).map(|o| o.choice),
        _ => None,
    });
    assert_eq!(g.player(0).graveyard.len(), 5);
    // A new object (exiled and returned), the planeswalker face up with its
    // loyalty (CR 306.5b).
    let j2 = bf(&g, 0, "Jace, Telepath Unbound");
    assert_eq!(j2, j);
    assert_ne!(g.obj(j2).inc, 0);
    assert!(g.is_type(j2, engine::db::types::PLANESWALKER) && !g.is_creature(j2));
    assert_eq!(loyalty(&g, j2), 5);
    assert_eq!(g.colors_of(j2), 1 << 1);
    assert_eq!(g.def_of(j2).mana_value(), 2);
}

#[test]
fn jace_with_fewer_than_five_stays() {
    let mut g = scen().battlefield(0, &[JACE]).graveyard(0, &["Island"; 2]).hand(0, &["Forest"]).start();
    let j = bf(&g, 0, JACE);
    g.choose(act(j, 0));
    settle(&mut g);
    assert_eq!(g.player(0).graveyard.len(), 3);
    assert_eq!(g.name(j), JACE);
}

#[test]
fn jace_telepath_unbound_abilities() {
    // "+1: Up to one target creature gets -2/-0 until your next turn."
    let mut g = scen().battlefield(0, &[JACE]).transformed(0, JACE).battlefield(1, &["Test Bear"]).start();
    let j = bf(&g, 0, "Jace, Telepath Unbound");
    let bear = bf(&g, 1, "Test Bear");
    g.choose(act(j, 0));
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert_eq!(g.pt(bear), (0, 2));
    assert_eq!(loyalty(&g, j), 6);
    // Until P0's next turn: still -2/-0 in P1's turn.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1);
    assert_eq!(g.pt(bear), (0, 2));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1);
    assert_eq!(g.pt(bear), (2, 2));
}

#[test]
fn jace_minus_three_casts_a_graveyard_spell_then_exiles_it() {
    // "-3: You may cast target instant or sorcery card from your graveyard
    // this turn. If that spell would be put into your graveyard, exile it
    // instead."
    let mut g = scen()
        .battlefield(0, &[JACE])
        .transformed(0, JACE)
        .counters(0, "Jace, Telepath Unbound", Counter::Loyalty, 5)
        .battlefield(0, &["Mountain"])
        .graveyard(0, &["Lightning Bolt"])
        .start();
    let j = bf(&g, 0, "Jace, Telepath Unbound");
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    g.choose(act(j, 1));
    g.choose(g.tgt(bolt));
    settle(&mut g);
    assert_eq!(loyalty(&g, j), 2);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.life(1), 17);
    assert_eq!(g.obj(bolt).zone, Zone::Exile, "exiled instead of the graveyard");
}

#[test]
fn jace_minus_three_spell_countered_to_hand_is_not_exiled() {
    // Only "put into your graveyard" is replaced (unlike flashback, CR
    // 702.34a): Remand sends it to its owner's hand.
    let mut g = scen()
        .battlefield(0, &[JACE])
        .transformed(0, JACE)
        .counters(0, "Jace, Telepath Unbound", Counter::Loyalty, 5)
        .battlefield(0, &["Mountain"])
        .graveyard(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"; 2])
        .hand(1, &["Remand"])
        .start();
    let j = bf(&g, 0, "Jace, Telepath Unbound");
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    g.choose(act(j, 1));
    g.choose(g.tgt(bolt));
    settle(&mut g);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    g.pass(); // P1 gets priority
    let remand = g.card(1, "Remand", Zone::Hand);
    g.choose(Choice::Cast(remand));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let spell = g.tgt(bolt);
    g.choose(spell);
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Hand);
    assert_eq!(g.life(1), 20);
}

#[test]
fn jace_minus_nine_emblem_mills_target_opponent_per_spell() {
    // "-9: You get an emblem with 'Whenever you cast a spell, target
    // opponent mills five cards.'"
    let mut g = scen()
        .battlefield(0, &[JACE])
        .transformed(0, JACE)
        .counters(0, "Jace, Telepath Unbound", Counter::Loyalty, 9)
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .start();
    let j = bf(&g, 0, "Jace, Telepath Unbound");
    g.choose(act(j, 2));
    settle(&mut g);
    assert_eq!(g.obj(j).zone, Zone::Graveyard, "0 loyalty (CR 704.5i)");
    assert_eq!(g.command_zone().len(), 1);
    cast(&mut g, 0, "Lightning Bolt");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().find(|o| o.choice == player(1)).map(|o| o.choice),
        _ => None,
    });
    assert_eq!(g.player(1).library.len(), 5);
    assert_eq!(g.player(1).graveyard.len(), 5);
}

// ---- Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar -----------------

const TAMIYO: &str = "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar";
const SCHOLAR: &str = "Tamiyo, Seasoned Scholar";

#[test]
fn tamiyo_investigates_when_she_attacks() {
    // "Flying. Whenever Tamiyo attacks, investigate."
    let mut g = scen().battlefield(0, &[TAMIYO]).at(Step::Main1, 0).start();
    let t = bf(&g, 0, TAMIYO);
    assert_eq!(g.pt(t), (0, 3));
    assert!(g.has_kw(t, Kw::Flying));
    attack(&mut g, &[t], |_| None);
    assert_eq!(g.count_in(0, "Clue", Zone::Battlefield), 1);
}

#[test]
fn tamiyo_flips_when_you_draw_your_third_card_in_a_turn() {
    // "When you draw your third card in a turn, exile Tamiyo, then return
    // her to the battlefield transformed under her owner's control."
    let mut g = scen()
        .battlefield(0, &[TAMIYO, "Island"])
        .hand(0, &["Ancestral Recall"])
        .start();
    cast(&mut g, 0, "Ancestral Recall");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(0)),
        _ => None,
    });
    let t = bf(&g, 0, SCHOLAR);
    assert!(g.is_type(t, engine::db::types::PLANESWALKER));
    assert_eq!(loyalty(&g, t), 2);
    assert_eq!(g.colors_of(t), (1 << 1) | (1 << 4), "blue and green");
}

#[test]
fn tamiyo_doesnt_flip_before_the_third_card() {
    let mut g = scen().battlefield(0, &[TAMIYO, "Island", "Island"]).hand(0, &["Consider"]).start();
    // Consider: surveil 1, then draw a card (one draw).
    cast(&mut g, 0, "Consider");
    settle(&mut g);
    assert_eq!(g.name(bf(&g, 0, TAMIYO)), TAMIYO);
}

#[test]
fn tamiyo_seasoned_scholar_plus_two_shrinks_attackers_until_your_next_turn() {
    // "+2: Until your next turn, whenever a creature attacks you or a
    // planeswalker you control, it gets -1/-0 until end of turn."
    let mut g = scen()
        .battlefield(0, &[TAMIYO])
        .transformed(0, TAMIYO)
        .counters(0, SCHOLAR, Counter::Loyalty, 2)
        .battlefield(1, &["Test Bear"])
        .start();
    let t = bf(&g, 0, SCHOLAR);
    g.choose(act(t, 0));
    settle(&mut g);
    assert_eq!(loyalty(&g, t), 4);
    // P1's turn: the Bear attacks P0 and is 1/2 this turn.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Card(bear));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(Choice::Target(Target::Player(0)));
    }
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.pt(bear), (1, 2));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(0), 19);
    // P0's next turn began: the ability is gone; the Bear attacks for 2.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().number > 4 && g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(Choice::Target(Target::Player(0)));
    }
    g.choose(Choice::Pass);
    assert!(g.stack().is_empty() && g.waiting_triggers().is_empty());
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(0), 17);
}

#[test]
fn tamiyo_minus_three_returns_an_instant_and_adds_mana_if_green() {
    // "-3: Return target instant or sorcery card from your graveyard to
    // your hand. If it's a green card, add one mana of any color."
    for (card, green) in [("Lightning Bolt", false), ("Crop Rotation", true)] {
        let mut g = scen()
            .battlefield(0, &[TAMIYO])
            .transformed(0, TAMIYO)
            .counters(0, SCHOLAR, Counter::Loyalty, 3)
            .graveyard(0, &[card])
            .start();
        let t = bf(&g, 0, SCHOLAR);
        let c = g.card(0, card, Zone::Graveyard);
        g.choose(act(t, 1));
        g.choose(g.tgt(c));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMana) => Some(Choice::Number(1)),
            _ => None,
        });
        assert_eq!(g.obj(c).zone, Zone::Hand);
        let total: u8 = g.player(0).pool.amt.iter().sum();
        assert_eq!(total, green as u8, "{card}");
    }
}

#[test]
fn tamiyo_minus_seven_draws_half_the_library_and_gives_no_maximum_hand_size() {
    // "-7: Draw cards equal to half the number of cards in your library,
    // rounded up. You get an emblem with 'You have no maximum hand size.'"
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 9])
        .library(1, &["Forest"; 10])
        .battlefield(0, &[TAMIYO])
        .transformed(0, TAMIYO)
        .counters(0, SCHOLAR, Counter::Loyalty, 7)
        .start();
    let t = bf(&g, 0, SCHOLAR);
    g.choose(act(t, 2));
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 5, "half of 9, rounded up");
    assert_eq!(g.command_zone().len(), 1);
    // Cleanup with 12 cards: no discard (CR 402.2, 514.1).
    let mut g2 = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 30])
        .library(1, &["Forest"; 10])
        .battlefield(0, &[TAMIYO])
        .transformed(0, TAMIYO)
        .counters(0, SCHOLAR, Counter::Loyalty, 7)
        .hand(0, &["Island"; 4])
        .start();
    let t = bf(&g2, 0, SCHOLAR);
    g2.choose(act(t, 2));
    settle(&mut g2);
    assert_eq!(g2.player(0).hand.len(), 19);
    quiet_until(&mut g2, |g| g.turn().active == 1);
    assert_eq!(g2.player(0).hand.len(), 19);
}

// ---- Etali, Primal Conqueror // Etali, Primal Sickness -----------------------

const ETALI: &str = "Etali, Primal Conqueror // Etali, Primal Sickness";

#[test]
fn etali_exiles_until_nonland_and_casts_them_free_in_any_order_cr_608_2g() {
    // "Trample. When Etali enters, each player exiles cards from the top of
    // their library until they exile a nonland card. You may cast any
    // number of spells from among the nonland cards exiled this way without
    // paying their mana costs."
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Lightning Bolt", "Island"])
        .library(1, &["Forest", "Forest", "Test Bear", "Forest"])
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &[ETALI])
        .start();
    cast(&mut g, 0, ETALI);
    // Any number, in any order: the Bear first, then the Bolt.
    let offered = std::cell::RefCell::new(Vec::new());
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseExiled) => {
            let opts = &g.pending_decision().unwrap().options;
            let names: Vec<String> = opts
                .iter()
                .filter_map(|o| match o.choice {
                    Choice::Card(c) => Some(g.name(c).to_string()),
                    _ => None,
                })
                .collect();
            offered.borrow_mut().push(names.clone());
            let want = if names.iter().any(|n| n == "Test Bear") { "Test Bear" } else { "Lightning Bolt" };
            opts.iter().find(|o| matches!(o.choice, Choice::Card(c) if g.name(c) == want)).map(|o| o.choice)
        }
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(
        *offered.borrow(),
        vec![vec!["Lightning Bolt".to_string(), "Test Bear".to_string()], vec!["Lightning Bolt".to_string()]]
    );
    let e = bf(&g, 0, ETALI);
    assert!(g.has_kw(e, Kw::Trample));
    assert_eq!(g.count_in(0, "Island", Zone::Exile), 1);
    assert_eq!(g.count_in(1, "Forest", Zone::Exile), 2);
    assert_eq!(g.life(1), 17, "the Bolt, free");
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1, "the Bear under its caster's control? its owner P1's card");
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    assert_eq!(g.obj(bear).controller, 0, "cast by P0: P0 controls it");
    assert_eq!(g.player(0).library.len(), 1);
}

#[test]
fn etali_may_cast_none() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Lightning Bolt", "Island"])
        .library(1, &["Test Bear", "Forest"])
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &[ETALI])
        .start();
    cast(&mut g, 0, ETALI);
    let asked = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseExiled) => {
            asked.set(asked.get() + 1);
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(asked.get(), 1, "PASS: no more");
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Exile), 1);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile), 1);
}

#[test]
fn etali_primal_sickness_poisons_and_ten_poison_counters_lose_cr_704_5c() {
    // "{9}{G/P}: Transform Etali. Activate only as a sorcery." // "Trample,
    // indestructible. Whenever Etali deals combat damage to a player, they
    // get that many poison counters."
    let mut g = scen().battlefield(0, &[ETALI]).battlefield(0, &["Forest"; 10]).at(Step::Main1, 0).start();
    let e = bf(&g, 0, ETALI);
    g.choose(act(e, 0));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.name(e), "Etali, Primal Sickness");
    assert_eq!(g.pt(e), (11, 11));
    assert!(g.has_kw(e, Kw::Indestructible) && g.has_kw(e, Kw::Trample));
    assert_eq!(g.colors_of(e), (1 << 3) | (1 << 4));
    attack(&mut g, &[e], |_| None);
    assert!(g.is_over());
    assert_eq!(g.outcome().unwrap().winner, Some(0));
    assert_eq!(g.player(1).poison, 11);
    assert_eq!(g.life(1), 9);
}

// ---- Outland Liberator // Frenzied Trapbreaker (day and night) ---------------

const LIBERATOR: &str = "Outland Liberator // Frenzied Trapbreaker";

#[test]
fn outland_liberator_makes_it_day_and_flips_at_night_cr_731_2() {
    // "{1}, Sacrifice this creature: Destroy target artifact or enchantment.
    // Daybound (If a player casts no spells during their own turn, it
    // becomes night next turn.)"
    let mut g = scen().battlefield(0, &["Forest", "Forest"]).hand(0, &[LIBERATOR]).start();
    assert_eq!(g.day(), None);
    cast(&mut g, 0, LIBERATOR);
    settle(&mut g);
    // CR 702.145d: it becomes day.
    assert_eq!(g.day(), Some(true));
    let l = bf(&g, 0, LIBERATOR);
    assert_eq!(g.pt(l), (2, 2));
    // P0 cast a spell this turn: P1's turn is still day. P1 casts nothing:
    // P0's next turn becomes night (CR 731.2a) and it transforms at once.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1);
    assert_eq!(g.day(), Some(true));
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Upkeep);
    assert_eq!(g.day(), Some(false));
    assert_eq!(g.name(l), "Frenzied Trapbreaker");
    assert_eq!(g.pt(l), (3, 3));
}

#[test]
fn frenzied_trapbreaker_flips_back_when_a_player_casts_two_spells_cr_731_2b() {
    // "Nightbound (If a player casts at least two spells during their own
    // turn, it becomes day next turn.)"
    let mut g = scen()
        .battlefield(0, &[LIBERATOR])
        .transformed(0, LIBERATOR)
        .day(Some(false))
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Lightning Bolt", "Lightning Bolt"])
        .start();
    let l = bf(&g, 0, "Frenzied Trapbreaker");
    for _ in 0..2 {
        let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(bolt));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(player(1));
        pay(&mut g);
        settle(&mut g);
    }
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Upkeep);
    assert_eq!(g.day(), Some(true));
    assert_eq!(g.name(l), LIBERATOR);
}

#[test]
fn outland_liberator_enters_transformed_at_night_cr_702_145b() {
    let mut g = scen().day(Some(false)).battlefield(0, &["Forest", "Forest"]).hand(0, &[LIBERATOR]).start();
    cast(&mut g, 0, LIBERATOR);
    settle(&mut g);
    assert_eq!(g.count_in(0, "Frenzied Trapbreaker", Zone::Battlefield), 1);
    assert_eq!(g.day(), Some(false));
}

#[test]
fn outland_liberator_and_frenzied_trapbreaker_destroy_artifacts() {
    // Front: "{1}, Sacrifice: destroy target artifact or enchantment."
    let mut g = scen().battlefield(0, &[LIBERATOR, "Forest"]).battlefield(1, &["Sol Ring"]).start();
    let l = bf(&g, 0, LIBERATOR);
    let ring = bf(&g, 1, "Sol Ring");
    g.choose(act(l, 0));
    g.choose(g.tgt(ring));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert_eq!(g.obj(l).zone, Zone::Graveyard);
    // Back: "Whenever this creature attacks, destroy target artifact or
    // enchantment defending player controls."
    let mut g = scen()
        .battlefield(0, &[LIBERATOR, "Sol Ring"])
        .transformed(0, LIBERATOR)
        .day(Some(false))
        .battlefield(1, &["Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    let t = bf(&g, 0, "Frenzied Trapbreaker");
    let stone = bf(&g, 1, "Mind Stone");
    let my_ring = bf(&g, 0, "Sol Ring");
    attack(&mut g, &[t], |g| match g.kind() {
        Some(DecisionKind::Target) => {
            assert!(!g.has_option(g.tgt(my_ring)), "defending player's only");
            Some(g.tgt(stone))
        }
        _ => None,
    });
    assert_eq!(g.obj(stone).zone, Zone::Graveyard);
}

// ---- Jennifer Walters // The Sensational She-Hulk (transform) -----------------

const JENNIFER: &str = "Jennifer Walters // The Sensational She-Hulk";
const SHE_HULK: &str = "The Sensational She-Hulk";

#[test]
fn jennifer_walters_stops_opponents_casting_in_your_turn_and_transforms() {
    // "Your opponents can't cast spells during your turn. {3}{G}{W}{W}:
    // Transform Jennifer Walters. Activate only as a sorcery."
    let mut g = scen()
        .battlefield(0, &[JENNIFER])
        .battlefield(0, &["Plains", "Plains", "Forest", "Forest", "Forest", "Forest"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    let j = bf(&g, 0, JENNIFER);
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    // Transform (the MDFC's back face: its own mana cost and mana value,
    // CR 712.8f).
    g.choose(act(j, 0));
    pay(&mut g);
    g.pass(); // P1 has priority in P0's turn
    assert!(!g.has_option(Choice::Cast(bolt)), "opponents can't cast spells during your turn");
    settle(&mut g);
    assert_eq!(g.name(j), SHE_HULK);
    assert_eq!(g.pt(j), (6, 6));
    assert!(g.has_kw(j, Kw::Reach) && g.has_kw(j, Kw::Trample));
    assert_eq!(g.def_of(j).mana_value(), 6);
    assert_eq!(g.colors_of(j), 1 | (1 << 4));
    // She-Hulk says the same.
    g.pass();
    assert!(!g.has_option(Choice::Cast(bolt)));
    // In P1's own turn P1 may cast.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(Choice::Cast(bolt)));
}
