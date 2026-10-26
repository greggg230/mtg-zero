//! M8d (ENGINE.md 37): the official rulings of the cube's protection and
//! targeting cards (Emrakul, the Aeons Torn; True-Name Nemesis; Mother of
//! Runes; Giver of Runes) as engine tests: protection (CR 702.16) on the
//! battlefield only (113.6), "can't be countered" (101.2), annihilator
//! (702.86), a spell or ability with all its targets illegal (608.2b).
//! `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

const EMRAKUL: &str = "Emrakul, the Aeons Torn";
const TNN: &str = "True-Name Nemesis";

fn act(g: &Game, c: CardIdx) -> Choice {
    (0..8u8)
        .map(|i| Choice::Ability(c, AbilitySlot::Activated(i)))
        .find(|&a| g.has_option(a))
        .unwrap_or_else(|| panic!("no ability of {} offered: {}", g.name(c), opts(g)))
}

// ---- Emrakul, the Aeons Torn -----------------------------------------------------------------

#[test]
fn emrakul_on_the_stack_may_be_targeted_by_a_counterspell_which_doesnt_counter_it_cr_101_2() {
    // ruling: f0f2746f
    // Protection works on the battlefield (CR 113.6): the blue Counterspell
    // may target the Emrakul spell; it resolves and does nothing.
    let mut g = scen()
        .battlefield(0, &["Island"; 15])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &[EMRAKUL])
        .hand(1, &["Counterspell"])
        .start();
    let e = g.card(0, EMRAKUL, Zone::Hand);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(e));
    g.drive_until(at_priority);
    // The cast trigger (an extra turn) is above the spell (CR 603.3).
    assert_eq!(g.stack().len(), 2);
    g.pass();
    g.choose(Choice::Cast(cs));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            // The Emrakul spell is the one target offered.
            let d = g.pending_decision().unwrap();
            assert_eq!(d.options.len(), 1, "{}", opts(g));
            assert!(opts(g).contains("Emrakul"), "{}", opts(g));
            Some(d.options[0].choice)
        }
        _ => None,
    });
    assert_eq!(g.obj(cs).zone, Zone::Graveyard, "Counterspell resolved");
    assert_eq!(g.obj(e).zone, Zone::Battlefield, "not countered");
}

#[test]
fn emrakul_can_be_blocked_by_colored_creatures_and_targeted_by_their_abilities_cr_702_16() {
    // ruling: 50fb7446
    // Ertai Resurrected (blue-black) entering: its triggered ability isn't a
    // spell, so it may target and destroy Emrakul; its controller draws.
    let mut g = scen()
        .battlefield(0, &[EMRAKUL])
        .battlefield(1, &["Island", "Island", "Swamp", "Swamp"])
        .hand(1, &["Ertai Resurrected"])
        .at(Step::Main1, 1)
        .start();
    let e = bf(&g, 0, EMRAKUL);
    let ertai = g.card(1, "Ertai Resurrected", Zone::Hand);
    let lib = g.player(0).library.len();
    g.choose(Choice::Cast(ertai));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            let i = g.options_desc().iter().position(|s| s.contains("estroy")).unwrap_or(1);
            Some(g.pending_decision().unwrap().options[i].choice)
        }
        Some(DecisionKind::Target) => {
            assert!(g.has_option(g.tgt(e)), "{}", opts(g));
            Some(g.tgt(e))
        }
        _ => None,
    });
    assert_ne!(g.obj(e).zone, Zone::Battlefield, "destroyed\n{}", g.log_lines().join("\n"));
    // "When Emrakul is put into a graveyard from anywhere, its owner
    // shuffles their graveyard into their library", and P0 drew one.
    assert_eq!(g.player(0).library.len(), lib + 1 - 1);
}

#[test]
fn emrakul_a_colored_creature_may_block_it_cr_702_16() {
    // ruling: 50fb7446
    // P1 has seven permanents: annihilator 6 takes six lands, the blue Test
    // Flyer blocks.
    let mut g = scen()
        .battlefield(0, &[EMRAKUL])
        .battlefield(1, &["Test Flyer", "Island", "Island", "Island", "Island", "Island", "Island"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let e = bf(&g, 0, EMRAKUL);
    let fl = bf(&g, 1, "Test Flyer");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(e));
    g.choose(Choice::Pass);
    quiet_until_with(
        &mut g,
        |g| g.turn().step == Step::Main2,
        |g| match g.kind() {
            Some(DecisionKind::Sacrifice) => {
                g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Card(fl))
            }
            Some(DecisionKind::DeclareBlockers) if g.has_option(Choice::Card(e)) => Some(Choice::Card(e)),
            _ => None,
        },
    );
    assert_eq!(g.obj(fl).zone, Zone::Graveyard, "the Flyer blocked and died\n{}", g.log_lines().join("\n"));
    assert_eq!(g.life(1), 20);
}

#[test]
fn emrakul_annihilator_sacrifices_before_blocks_and_a_sacrificed_planeswalker_leaves_it_attacking_cr_702_86a_506_4() {
    // ruling: 87952c85 8be24dfb
    // Emrakul attacks Jace; P1 has exactly six permanents (Jace, a Test
    // Flyer, four Islands) and sacrifices them all: no blocker left, and
    // Emrakul, still attacking, deals no combat damage.
    let mut g = scen()
        .battlefield(0, &[EMRAKUL])
        .battlefield(1, &["Jace, the Mind Sculptor", "Test Flyer", "Island", "Island", "Island", "Island"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let e = bf(&g, 0, EMRAKUL);
    let jace = bf(&g, 1, "Jace, the Mind Sculptor");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(e));
    g.choose(g.tgt(jace));
    g.choose(Choice::Pass);
    let mut blockers_asked = false;
    for _ in 0..200 {
        if g.turn().step == Step::Main2 {
            break;
        }
        if g.kind() == Some(DecisionKind::DeclareBlockers) {
            blockers_asked = true;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        if d.kind == DecisionKind::Priority && g.turn().step == Step::CombatDamage {
            assert!(g.obj(e).attacking, "still attacking");
        }
        g.choose(c);
    }
    assert!(!blockers_asked, "no creature left to block");
    assert_eq!(g.permanents_of(1).count(), 0);
    assert_eq!(g.obj(jace).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "no combat damage to anything");
}

// ---- True-Name Nemesis -----------------------------------------------------------------------

#[test]
fn true_name_nemesis_has_protection_from_everything_the_chosen_player_controls_cr_702_16j() {
    // ruling: d873839e
    // Colorless objects too: P1's Walking Ballista's ability can't target it
    // (but can target P0's Test Bear), and P1's colorless Haywire Mite can't
    // block it.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Test Bear"])
        .battlefield(1, &["Walking Ballista"])
        .counters(1, "Walking Ballista", Counter::P1P1, 2)
        .hand(0, &[TNN])
        .start();
    let t = g.card(0, TNN, Zone::Hand);
    g.choose(Choice::Cast(t));
    resolve_with(&mut g, |g| match g.kind() {
        _ if g.has_option(player(1)) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.obj(t).zone, Zone::Battlefield);
    assert_eq!(g.obj(t).chosen_player, Some(1), "{}", g.log_lines().join("\n"));
    let bear = bf(&g, 0, "Test Bear");
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let wb = bf(&g, 1, "Walking Ballista");
    g.choose(act(&g, wb));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", opts(&g));
    assert!(!g.has_option(g.tgt(t)), "protection from P1's colorless ability");
    assert!(g.has_option(g.tgt(bear)));
}

#[test]
fn true_name_nemesis_cant_be_blocked_by_the_chosen_players_colorless_creature_cr_702_16() {
    // ruling: d873839e
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island"])
        .battlefield(1, &["Haywire Mite"])
        .hand(0, &[TNN])
        .start();
    let t = g.card(0, TNN, Zone::Hand);
    g.choose(Choice::Cast(t));
    resolve_with(&mut g, |g| match g.kind() {
        _ if g.has_option(player(1)) => Some(player(1)),
        _ => None,
    });
    // Next turn of P0: attack; the Mite isn't offered a block.
    let blocked = std::cell::Cell::new(false);
    quiet_until_with(
        &mut g,
        |g| g.turn().number == 5 && g.turn().step == Step::Main2,
        |g| match g.kind() {
            Some(DecisionKind::DeclareAttackers) if g.turn().number == 5 && g.has_option(Choice::Card(t)) => Some(Choice::Card(t)),
            Some(DecisionKind::DeclareBlockers) => {
                blocked.set(blocked.get() || g.has_option(Choice::Card(t)));
                Some(Choice::Pass)
            }
            _ => None,
        },
    );
    assert!(!blocked.get(), "the Mite can't block it");
    assert_eq!(g.life(1), 17, "unblocked\n{}", g.log_lines().join("\n"));
}

// ---- Mother of Runes and Giver of Runes ------------------------------------------------------

#[test]
fn a_target_made_illegal_by_protection_makes_the_ability_do_nothing_cr_608_2b() {
    // ruling: 5eda3731
    // Ertai Resurrected's "destroy another target creature; its controller
    // draws a card": Mother of Runes gives the target protection from blue
    // in response; the ability's only target is illegal, so nothing happens,
    // not even the draw.
    let mut g = scen()
        .battlefield(0, &["Mother of Runes", "Test Bear"])
        .battlefield(1, &["Island", "Island", "Swamp", "Swamp"])
        .hand(1, &["Ertai Resurrected"])
        .at(Step::Main1, 1)
        .start();
    let mom = bf(&g, 0, "Mother of Runes");
    let bear = bf(&g, 0, "Test Bear");
    let ertai = g.card(1, "Ertai Resurrected", Zone::Hand);
    let lib = g.player(0).library.len();
    g.choose(Choice::Cast(ertai));
    // P1 casts; both pass; Ertai enters; its trigger targets the Bear.
    for _ in 0..100 {
        if at_priority(&g) && g.stack().iter().any(|s| !matches!(s.kind, engine::game::StackKind::Spell { .. })) {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::ChooseMode => {
                let i = g.options_desc().iter().position(|s| s.contains("estroy")).unwrap_or(1);
                d.options[i].choice
            }
            DecisionKind::Target => g.tgt(bear),
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    // P0 responds with Mother of Runes: protection from blue.
    if g.pending_decision().unwrap().player == 1 {
        g.pass();
    }
    g.choose(act(&g, mom));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(1)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_eq!(g.player(0).library.len(), lib, "no card drawn\n{}", g.log_lines().join("\n"));
}

#[test]
fn protection_from_colorless_unattaches_a_colorless_equipment_cr_702_16c_704_5n() {
    // ruling: 26c0eed3
    let mut g = scen().battlefield(0, &["Giver of Runes", "Skullclamp", "Test Bear", "Island"]).start();
    let giver = bf(&g, 0, "Giver of Runes");
    let clamp = bf(&g, 0, "Skullclamp");
    let bear = bf(&g, 0, "Test Bear");
    g.choose(act(&g, clamp));
    let t = g.tgt(bear);
    answer_targets(&mut g, &[t]);
    assert_eq!(g.pt(bear), (3, 1));
    g.choose(act(&g, giver));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(5)),
        _ => None,
    });
    assert!(g.protection_of(bear).colorless);
    assert_eq!(g.obj(clamp).attached, None, "can't be equipped by colorless Equipment");
    assert_eq!(g.pt(bear), (2, 2));
}

#[test]
fn protection_from_a_color_stops_blocks_by_that_color_and_its_damage_cr_702_16b_702_16e() {
    // ruling: b0d58e2b
    // Mother of Runes gives P0's attacking Test Bear protection from green:
    // P1's green Test Bear can't block it; P1's red Lightning Bolt can still
    // target it; damage from green sources would be prevented.
    let mut g = scen()
        .battlefield(0, &["Mother of Runes", "Test Bear"])
        .battlefield(1, &["Test Bear"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let mom = bf(&g, 0, "Mother of Runes");
    let bear = bf(&g, 0, "Test Bear");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    g.choose(act(&g, mom));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseColor) => Some(Choice::Number(4)),
        _ => None,
    });
    let could_block = std::cell::Cell::new(false);
    quiet_until_with(
        &mut g,
        |g| g.turn().step == Step::Main2,
        |g| {
            if g.kind() == Some(DecisionKind::DeclareBlockers) && g.has_option(Choice::Card(bear)) {
                could_block.set(true);
            }
            None
        },
    );
    assert!(!could_block.get(), "a green creature can't block it");
    assert_eq!(g.life(1), 18, "unblocked
{}", g.log_lines().join("
"));
}
