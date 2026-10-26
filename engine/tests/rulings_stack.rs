//! M8d (ENGINE.md 37): the official rulings of the cube's stack cards
//! (Tishana's Tidebinder, Ertai Resurrected) as engine tests: countering an
//! ability (701.5), an effect that lasts "for as long as" (611.2b), the
//! layer-6 timestamp order of an ability loss and a later grant (613.7),
//! what a resolving ability checks (608.2), a countered delayed trigger
//! (603.7). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter, Kw};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;

const TIDEBINDER: &str = "Tishana's Tidebinder";
const BALLISTA: &str = "Walking Ballista";

fn stack_target(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Target(Target::Stack(_))))
}

fn act(g: &Game, c: CardIdx) -> Choice {
    (0..8u8)
        .map(|i| Choice::Ability(c, AbilitySlot::Activated(i)))
        .find(|&a| g.has_option(a))
        .unwrap_or_else(|| panic!("no ability of {} offered: {}", g.name(c), opts(g)))
}

/// P0's Walking Ballista pings P1 (its ability on the stack); P1 casts
/// Tishana's Tidebinder, which enters; its trigger targets the ping and
/// waits on the stack with P0 to act. Returns (Ballista, Tidebinder).
fn tidebinder_trigger_on_the_ping(g: &mut Game) -> (CardIdx, CardIdx) {
    let wb = bf(g, 0, BALLISTA);
    g.choose(Choice::Ability(wb, AbilitySlot::Activated(1)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    g.drive_until(at_priority);
    g.pass();
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    for _ in 0..100 {
        let trig_up = g.stack().len() == 2 && g.obj(tb).zone == Zone::Battlefield;
        if trig_up && at_priority(g) && g.pending_decision().unwrap().player == 0 {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::Target => stack_target(g).unwrap(),
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.stack().len(), 2, "the ping and the trigger\n{}", g.log_lines().join("\n"));
    (wb, tb)
}

#[test]
fn tidebinder_gone_before_its_trigger_resolves_still_counters_but_nothing_loses_abilities_cr_611_2b() {
    // ruling: da0512a5
    let mut g = scen()
        .battlefield(0, &[BALLISTA, "Mountain"])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &[TIDEBINDER])
        .start();
    let (wb, tb) = tidebinder_trigger_on_the_ping(&mut g);
    // P0 Bolts the Tidebinder (3/2) in response.
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(tb));
    resolve(&mut g);
    assert_eq!(g.obj(tb).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "the ping was still countered");
    assert!(g.has_option(Choice::Ability(wb, AbilitySlot::Activated(1))), "no loss of abilities: {}", opts(&g));
}

#[test]
fn tidebinder_an_ability_gained_after_the_loss_is_kept_cr_613_7() {
    // ruling: 6cadb1cc
    // After the Ballista loses all abilities, Lightning Greaves is attached
    // to it: haste and shroud, from a later timestamp, apply.
    let mut g = scen()
        .battlefield(0, &[BALLISTA, "Lightning Greaves"])
        .counters(0, BALLISTA, Counter::P1P1, 2)
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &[TIDEBINDER])
        .start();
    let (wb, _tb) = tidebinder_trigger_on_the_ping(&mut g);
    resolve(&mut g);
    assert!(!g.has_option(Choice::Ability(wb, AbilitySlot::Activated(1))), "lost its abilities");
    let gr = bf(&g, 0, "Lightning Greaves");
    g.choose(act(&g, gr));
    let t = g.tgt(wb);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.obj(gr).attached.map(|h| h.idx), Some(wb));
    assert!(g.has_kw(wb, Kw::Haste) && g.has_kw(wb, Kw::Shroud), "{:?}", g.keywords_of(wb));
}

#[test]
fn tidebinder_checks_the_sources_types_as_its_trigger_resolves_cr_608_2() {
    // ruling: 4ff964f0
    // Celestial Colonnade's "becomes a creature" ability, countered: the
    // Colonnade is a land (not an artifact, creature or planeswalker) as the
    // trigger resolves, so it keeps its abilities.
    let mut g = scen()
        .battlefield(0, &["Celestial Colonnade", "Plains", "Plains", "Plains", "Island", "Island", "Island", "Island", "Plains", "Plains"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &[TIDEBINDER])
        .start();
    let col = bf(&g, 0, "Celestial Colonnade");
    g.choose(act(&g, col));
    // Pay with the other lands; then P1 casts the Tidebinder.
    for _ in 0..50 {
        if at_priority(&g) && g.pending_decision().unwrap().player == 1 {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::PayMana => d.options.iter().find(|o| !matches!(o.choice, Choice::Ability(c, _) if c == col)).unwrap().choice,
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.stack().len(), 1, "the animation on the stack");
    let tb = g.card(1, TIDEBINDER, Zone::Hand);
    g.choose(Choice::Cast(tb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => stack_target(g),
        _ => None,
    });
    assert_eq!(g.count_in(1, TIDEBINDER, Zone::Battlefield), 1);
    assert!(!g.is_creature(col), "the animation was countered");
    // It still has its abilities: animating it again is offered.
    assert!((0..4u8).any(|i| g.has_option(Choice::Ability(col, AbilitySlot::Activated(i)))), "{}", opts(&g));
}

#[test]
fn a_countered_next_end_step_delayed_trigger_doesnt_trigger_again_cr_603_7c() {
    // ruling: 4e573280
    // Memory Jar's "at the beginning of the next end step, each player
    // discards their hand and returns ... each card they exiled this way",
    // countered by Tishana's Tidebinder: the exiled cards stay exiled, also
    // after the following end steps.
    // P1's library is all Tidebinders: it draws seven after the Jar.
    let mut g = scen()
        .battlefield(0, &["Memory Jar"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt"])
        .hand(1, &["Test Giant"])
        .library(0, &["Island"; 20])
        .library(1, &[TIDEBINDER; 20])
        .start();
    let jar = bf(&g, 0, "Memory Jar");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let giant = g.card(1, "Test Giant", Zone::Hand);
    g.choose(act(&g, jar));
    resolve(&mut g);
    assert_eq!(g.player(1).hand.len(), 7);
    // P0's end step: the delayed trigger on the stack; P1 answers it.
    let countered = std::cell::Cell::new(false);
    quiet_until_with(
        &mut g,
        |g| g.turn().active == 1 && g.turn().step == Step::Main1,
        |g| {
            let d = g.pending_decision().unwrap();
            if d.kind == DecisionKind::Priority && d.player == 1 && g.turn().step == Step::End && !g.stack().is_empty() && !countered.get() {
                countered.set(true);
                return d.options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Cast(_)));
            }
            if d.kind == DecisionKind::Target {
                return stack_target(g);
            }
            None
        },
    );
    assert!(countered.get());
    assert_eq!(g.obj(bolt).zone, Zone::FaceDown, "never returned
{}", g.log_lines().join("
"));
    assert_eq!(g.obj(giant).zone, Zone::FaceDown);
    assert!(g.player(0).hand.len() >= 7, "nothing discarded");
    // Nor at the following end step.
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1);
    assert_eq!(g.obj(bolt).zone, Zone::FaceDown);
    assert_eq!(g.obj(giant).zone, Zone::FaceDown);
}
