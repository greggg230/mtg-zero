//! M8d part 3 (ENGINE.md 39): the cube's planeswalker rulings left after
//! the triage was narrowed: loyalty abilities of a planeswalker that
//! entered mid-combat wait for a main phase (CR 606.3), Comet, Stellar
//! Pup's die results. `ruling: <hash8>` names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter};
use engine::hidden::{key, KEY_DIE};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ab(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

#[test]
fn a_planeswalker_entering_in_combat_waits_for_a_main_phase_and_comets_die_cr_606_3() {
    // ruling: e5b123ca fee44c31 23ff8ba2 97f29a80
    // Ajani, Nacatl Pariah's Cat Warrior dies in combat: Ajani transforms;
    // his loyalty abilities wait for the second main phase. Comet with one
    // loyalty counter rolls 4: 1 damage, then the counter goes (he dies).
    // Comet rolls 3 with no card of mana value 2 or less in the graveyard:
    // he just loses a loyalty counter.
    let ajani = "Ajani, Nacatl Pariah // Ajani, Nacatl Avenger";
    let mut g = scen().battlefield(0, &[ajani, "Test Cat"]).battlefield(1, &["Test Giant"]).at(Step::DeclareAttackers, 0).start();
    let cat = bf(&g, 0, "Test Cat");
    let aj = bf(&g, 0, ajani);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(cat));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    let giant = bf(&g, 1, "Test Giant");
    let mut checked = false;
    for _ in 0..300 {
        if g.turn().step == Step::Main2 {
            break;
        }
        let d = g.pending_decision().unwrap();
        if d.kind == DecisionKind::Priority && d.player == 0 && g.name(aj) == "Ajani, Nacatl Avenger" {
            assert!((0..4u8).all(|i| !g.has_option(ab(aj, i))), "not in combat");
            checked = true;
        }
        let c = match d.kind {
            DecisionKind::DeclareBlockers if g.has_option(Choice::Card(giant)) => Choice::Card(giant),
            DecisionKind::DeclareBlockers if g.has_option(Choice::Card(cat)) => Choice::Card(cat),
            DecisionKind::YesNo => Choice::Yes,
            DecisionKind::Priority | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert!(checked, "Ajani transformed in combat\n{}", log(&g));
    g.drive_until(|g| at_priority(g) && g.turn().step == Step::Main2);
    assert!((0..4u8).any(|i| g.has_option(ab(aj, i))), "the main phase: {}", opts(&g));
    // Comet.
    let comet = "Comet, Stellar Pup";
    for (roll, loyalty, life1, gone) in [(4u64, 1u32, 19, true), (3, 3, 20, false)] {
        let mut g = scen().battlefield(0, &[comet]).counters(0, comet, Counter::Loyalty, loyalty).at(Step::Main1, 0).start();
        let c = bf(&g, 0, comet);
        g.choose(ab(c, 0));
        g.pass();
        g.force_chance(&[key(KEY_DIE, 0, roll)]);
        g.pass();
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(player(1)),
            _ => None,
        });
        assert_eq!(g.life(1), life1, "roll {roll}\n{}", log(&g));
        if gone {
            assert_eq!(g.obj(c).zone, Zone::Graveyard, "its last counter removed");
        } else {
            assert_eq!(g.counters_on(c, Counter::Loyalty), loyalty - 1, "roll 3, nothing to return: one counter\n{}", log(&g));
        }
    }
}
