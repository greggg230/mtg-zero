//! M8d (ENGINE.md 37): the official rulings of the cube's planeswalkers as
//! engine tests: loyalty abilities the turn one enters (CR 606.3), a cost
//! paid after the targets are chosen (602.2b, 601.2c), a planeswalker that
//! is a creature (Kaito: 306.8, 120.3), a loyalty ability that adds mana
//! uses the stack (605.1a). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

const JACE_VP: &str = "Jace, Vryn's Prodigy // Jace, Telepath Unbound";
const TAMIYO: &str = "Tamiyo, Collector of Tales";
const KAITO: &str = "Kaito, Bane of Nightmares";

fn loyalty_offered(g: &Game, c: CardIdx) -> bool {
    (0..4u8).any(|i| g.has_option(Choice::Ability(c, AbilitySlot::Activated(i))))
}

#[test]
fn jace_transformed_in_the_upkeep_activates_loyalty_only_in_a_main_phase_cr_606_3() {
    // ruling: 14835878 527934db
    // Jace, Vryn's Prodigy loots in P0's upkeep with four cards in the
    // graveyard: the discard makes five, he is exiled and returns
    // transformed. His loyalty abilities wait for the main phase (the turn
    // he entered is fine).
    let mut g = scen()
        .battlefield(0, &[JACE_VP])
        .graveyard(0, &["Island", "Island", "Island", "Island"])
        .hand(0, &["Forest"])
        .at(Step::Upkeep, 0)
        .start();
    g.drive_until(at_priority);
    assert_eq!(g.turn().step, Step::Upkeep);
    let j = bf(&g, 0, JACE_VP);
    g.choose(Choice::Ability(j, AbilitySlot::Activated(0)));
    resolve(&mut g);
    let tu = bf(&g, 0, "Jace, Telepath Unbound");
    assert_eq!(g.counters_on(tu, Counter::Loyalty), 5, "enters with its loyalty");
    assert_eq!(g.turn().step, Step::Upkeep);
    assert!(!loyalty_offered(&g, tu), "not in the upkeep");
    quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    assert!(loyalty_offered(&g, tu), "{}", opts(&g));
}

#[test]
fn tamiyo_with_three_loyalty_cant_return_herself_cr_602_2b() {
    // ruling: 086988d9
    // Her -3's target is chosen before the cost is paid (CR 601.2c, 602.2b):
    // she isn't in the graveyard then.
    let mut g = scen().battlefield(0, &[TAMIYO]).counters(0, TAMIYO, Counter::Loyalty, 3).graveyard(0, &["Island"]).start();
    let t = bf(&g, 0, TAMIYO);
    let island = g.card(0, "Island", Zone::Graveyard);
    g.choose(Choice::Ability(t, AbilitySlot::Activated(1)));
    let mut targets = Vec::new();
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Target) {
            assert!(!g.has_option(g.tgt(t)), "{}", opts(g));
        }
        None
    });
    targets.push(island);
    assert_eq!(g.obj(island).zone, Zone::Hand);
    assert_eq!(g.obj(t).zone, Zone::Graveyard, "0 loyalty (CR 704.5i)");
}

#[test]
fn kaito_as_a_creature_is_dealt_damage_not_loyalty_loss_cr_306_8() {
    // ruling: f0224bc7
    // On P0's turn Kaito is a 3/4 Ninja creature; blocked by a Test Bear he
    // is dealt 2 damage, marked, and keeps his loyalty.
    let mut g = scen().battlefield(0, &[KAITO]).battlefield(1, &["Test Bear"]).at(Step::DeclareAttackers, 0).start();
    let k = bf(&g, 0, KAITO);
    assert!(g.is_creature(k));
    let loyalty = g.counters_on(k, Counter::Loyalty);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(k));
    g.choose(Choice::Pass);
    quiet_until_with(
        &mut g,
        |g| g.turn().step == Step::Main2,
        |g| (g.kind() == Some(DecisionKind::DeclareBlockers)).then_some(Choice::Card(k)),
    );
    assert_eq!(g.obj(k).zone, Zone::Battlefield);
    assert_eq!(g.counters_on(k, Counter::Loyalty), loyalty, "{}", g.log_lines().join("\n"));
    assert_eq!(g.obj(k).damage, 2);
}

#[test]
fn chandras_mana_ability_is_a_loyalty_ability_on_the_stack_cr_606_605_1a() {
    // ruling: c85087fd
    let mut g = scen().battlefield(0, &["Chandra, Torch of Defiance"]).start();
    let c = bf(&g, 0, "Chandra, Torch of Defiance");
    // "+1: Add {R}{R}." is offered at priority and goes on the stack.
    // (Its second loyalty ability, slot 1.)
    let slot = 1u8;
    assert!(g.has_option(Choice::Ability(c, AbilitySlot::Activated(slot))), "{}", opts(&g));
    g.choose(Choice::Ability(c, AbilitySlot::Activated(slot)));
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 1, "it uses the stack");
    resolve(&mut g);
    assert_eq!(g.player(0).pool.amt[3], 2, "{{R}}{{R}}");
    // Once per turn.
    assert!(!loyalty_offered(&g, c));
}
