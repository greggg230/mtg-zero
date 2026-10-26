//! M8d part 2 (ENGINE.md 38): the riskiest rulings left after part 1 (37.7)
//! about the stack: damage divided as a spell is cast or a trigger is put on
//! the stack, with a target gone illegal (CR 601.2d, 603.3d, 608.2b: Fury,
//! Pyrokinesis, Fire Covenant, Shatterskull Smashing); a spell whose control
//! and targets change (115.7: Invert Polarity's variable targets and fixed
//! division); copies of modal, spree, X and divided spells (707.10: Lutri,
//! the Spellchaser copying Fiery Confluence, Mystic Confluence, Three Steps
//! Ahead, Fire Covenant, Brainstorm). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::hidden::{key, KEY_COIN};
use engine::{CardIdx, Choice, DecisionKind, Game, Target, Zone};
use rulings_util::*;
use std::cell::Cell;

const SMASHING: &str = "Shatterskull Smashing // Shatterskull, the Hammer Pass";
const LUTRI: &str = "Lutri, the Spellchaser";

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn spell(g: &Game, c: CardIdx) -> Choice {
    Choice::Target(Target::Obj(g.obj_ref(c)))
}

/// The caster's priority again after a cast (every cost decision answered
/// with its first option).
fn cast_done(g: &mut Game) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    panic!("cast_done: no progress\n{}", log(g));
}

/// Cast Fire Covenant ("As an additional cost to cast this spell, pay X
/// life. Fire Covenant deals X damage divided as you choose among any number
/// of target creatures.") with X = `x` at `targets`, giving the first
/// target `first` (the rest to the second); back at priority.
fn fire_covenant(g: &mut Game, x: u8, targets: &[CardIdx], first: u8) -> CardIdx {
    let c = g.card(0, "Fire Covenant", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(x));
    for &t in targets {
        assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", opts(g));
        g.choose(g.tgt(t));
    }
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Pass);
    }
    if targets.len() > 1 {
        assert_eq!(g.kind(), Some(DecisionKind::DivideDamage), "{}", opts(g));
        g.choose(Choice::Number(first));
    }
    cast_done(g);
    c
}

// ---- divided damage (CR 601.2d, 608.2b) --------------------------------------------------------

#[test]
fn fury_divides_as_its_trigger_is_put_on_the_stack_at_least_one_each_cr_603_3d_601_2d() {
    // ruling: 17a878c0
    // Fury's "When this creature enters, it deals 4 damage divided as you
    // choose among any number of target creatures and/or planeswalkers": the
    // division is made with the targets, the trigger not yet resolved, and
    // each target gets at least 1 (three targets: 1 or 2 to the first).
    let mut g = scen()
        .battlefield(0, &["Mountain"; 5])
        .battlefield(1, &["Test Bear", "Test Giant", "Test Wall"])
        .hand(0, &["Fury"])
        .start();
    let (bear, giant, wall) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"), bf(&g, 1, "Test Wall"));
    let fury = g.card(0, "Fury", Zone::Hand);
    g.choose(Choice::Cast(fury));
    // The mana cost (not evoke: no other red card in hand).
    quiet_until_with(&mut g, |g| g.kind() == Some(DecisionKind::Target), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        _ => None,
    });
    assert_eq!(g.obj(fury).zone, Zone::Battlefield);
    g.choose(g.tgt(bear));
    g.choose(g.tgt(giant));
    g.choose(g.tgt(wall));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Pass);
    }
    assert_eq!(g.kind(), Some(DecisionKind::DivideDamage), "{}", opts(&g));
    assert!(g.has_option(Choice::Number(1)) && g.has_option(Choice::Number(2)));
    assert!(!g.has_option(Choice::Number(0)) && !g.has_option(Choice::Number(3)), "at least 1 for each other target: {}", opts(&g));
    g.choose(Choice::Number(1));
    assert_eq!(g.kind(), Some(DecisionKind::DivideDamage));
    assert!(!g.has_option(Choice::Number(3)), "the wall needs 1: {}", opts(&g));
    g.choose(Choice::Number(2));
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 1, "the trigger waits on the stack, divided");
    assert_eq!(g.stack()[0].division, vec![1, 2, 1]);
    resolve(&mut g);
    assert_eq!((g.obj(bear).damage, g.obj(giant).damage, g.obj(wall).damage), (1, 2, 1), "{}", log(&g));
}

#[test]
fn fury_a_target_gone_illegal_gets_nothing_and_the_others_keep_their_shares_cr_608_2b() {
    // ruling: 45abd910
    // 2 to the Bear, 2 to the Giant; P0 Bolts the Bear with the trigger on
    // the stack: the Giant is dealt 2, not 4.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .hand(0, &["Fury", "Lightning Bolt"])
        .start();
    let (bear, giant) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"));
    let fury = g.card(0, "Fury", Zone::Hand);
    g.choose(Choice::Cast(fury));
    quiet_until_with(&mut g, |g| g.kind() == Some(DecisionKind::Target), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        _ => None,
    });
    g.choose(g.tgt(bear));
    g.choose(g.tgt(giant));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Pass);
    }
    g.choose(Choice::Number(2));
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.stack().len(), 1);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, bolt, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(giant).damage, 2, "{}", log(&g));
}

#[test]
fn pyrokinesis_divides_as_cast_and_an_illegal_target_gets_none_cr_601_2d_608_2b() {
    // ruling: d70bd868 7ea4dd5a
    // "Pyrokinesis deals 4 damage divided as you choose among any number of
    // target creatures." Cast for its mana cost: 1 to the Bear, 3 to the
    // Giant (each at least 1: no 0, no 4); the Bear Bolted in response: the
    // Giant is still dealt 3.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 7])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .hand(0, &["Pyrokinesis", "Lightning Bolt"])
        .start();
    let (bear, giant) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"));
    let p = g.card(0, "Pyrokinesis", Zone::Hand);
    g.choose(Choice::Cast(p));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    g.choose(g.tgt(bear));
    g.choose(g.tgt(giant));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Pass);
    }
    assert_eq!(g.kind(), Some(DecisionKind::DivideDamage), "divided as it is cast: {}", opts(&g));
    assert!(!g.has_option(Choice::Number(0)) && !g.has_option(Choice::Number(4)));
    g.choose(Choice::Number(1));
    cast_done(&mut g);
    assert_eq!(g.stack()[0].division, vec![1, 3]);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, bolt, &[t]);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(giant).damage, 3, "{}", log(&g));
    assert_eq!(g.obj(p).zone, Zone::Graveyard);
}

#[test]
fn pyrokinesis_with_every_target_illegal_doesnt_resolve_cr_608_2b() {
    // ruling: d70bd868
    let mut g = scen()
        .battlefield(0, &["Mountain"; 7])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Pyrokinesis", "Lightning Bolt"])
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let p = g.card(0, "Pyrokinesis", Zone::Hand);
    g.choose(Choice::Cast(p));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    g.choose(g.tgt(bear));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Pass);
    }
    cast_done(&mut g);
    assert_eq!(g.stack()[0].division, vec![4], "one target: all 4");
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, bolt, &[t]);
    assert!(log(&g).contains("fizzles"), "{}", log(&g));
    assert_eq!(g.obj(p).zone, Zone::Graveyard);
}

#[test]
fn fire_covenant_with_x_zero_has_no_targets_cr_601_2d() {
    // ruling: 79c9546a
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain"])
        .battlefield(1, &["Test Bear"])
        .hand(0, &["Fire Covenant"])
        .start();
    let c = g.card(0, "Fire Covenant", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert!(g.has_option(Choice::Number(0)), "{}", opts(&g));
    g.choose(Choice::Number(0));
    assert_ne!(g.kind(), Some(DecisionKind::Target), "no target can get at least 1: {}", opts(&g));
    cast_done(&mut g);
    assert!(g.stack()[0].targets.is_empty());
    resolve(&mut g);
    assert_eq!((g.life(0), g.obj(bf(&g, 1, "Test Bear")).damage), (20, 0));
}

#[test]
fn fire_covenant_division_isnt_redistributed_when_a_target_goes_cr_601_2d_608_2b() {
    // ruling: 77f08ae0 5f015d91
    // X = 3: 2 to the Bear, 1 to the Giant (a share of 0 or 3 can't be
    // chosen); the Bear Bolted in response: the Giant is dealt 1.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain", "Mountain"])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .hand(0, &["Fire Covenant", "Lightning Bolt"])
        .start();
    let (bear, giant) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"));
    let c = g.card(0, "Fire Covenant", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(3));
    g.choose(g.tgt(bear));
    g.choose(g.tgt(giant));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(Choice::Pass);
    }
    assert_eq!(g.kind(), Some(DecisionKind::DivideDamage));
    assert!(!g.has_option(Choice::Number(0)) && !g.has_option(Choice::Number(3)), "{}", opts(&g));
    g.choose(Choice::Number(2));
    cast_done(&mut g);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, bolt, &[t]);
    assert_eq!(g.obj(giant).damage, 1, "{}", log(&g));
    assert_eq!(g.life(0), 17);
}

#[test]
fn shatterskull_smashing_an_illegal_target_gets_nothing_the_other_its_share_cr_608_2b() {
    // ruling: 7afb4bdb f85041f1
    // X = 3: 2 to the Bear, 1 to the Giant; the Bear Bolted in response: the
    // Giant is dealt 1.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(1, &["Test Bear", "Test Giant"])
        .hand(0, &[SMASHING, "Lightning Bolt"])
        .lands_played(0, 1)
        .start();
    let (bear, giant) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"));
    let s = g.card(0, SMASHING, Zone::Hand);
    let face = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| match *c {
        Choice::Cast(x) | Choice::CastFace(x) | Choice::Faces(x, _) => x == s,
        _ => false,
    });
    g.choose(face.unwrap_or_else(|| panic!("{}", opts(&g))));
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(0));
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(3));
    g.choose(g.tgt(bear));
    g.choose(g.tgt(giant));
    assert_eq!(g.kind(), Some(DecisionKind::DivideDamage), "{}", opts(&g));
    assert!(!g.has_option(Choice::Number(0)) && !g.has_option(Choice::Number(3)));
    g.choose(Choice::Number(2));
    cast_done(&mut g);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    let t = g.tgt(bear);
    cast_targets(&mut g, bolt, &[t]);
    assert_eq!(g.obj(giant).damage, 1, "{}", log(&g));
}

// ---- Invert Polarity (CR 115.7) -----------------------------------------------------------

#[test]
fn invert_polarity_changes_targets_not_their_number_or_the_division_cr_115_7_601_2d() {
    // ruling: 747157aa c3434e5c
    // P0's Fire Covenant, X = 3: 2 to P1's Bear, 1 to P1's Giant. P1 steals
    // it and may choose a new target for each of the two (one decision per
    // target; none added or removed): the Bear's 2 goes to P0's own Bear,
    // the Giant keeps its 1.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain", "Test Bear"])
        .battlefield(1, &["Test Bear", "Test Giant", "Island", "Island", "Mountain"])
        .hand(0, &["Fire Covenant"])
        .hand(1, &["Invert Polarity"])
        .start();
    let (bear1, giant) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"));
    let bear0 = bf(&g, 0, "Test Bear");
    let fc = fire_covenant(&mut g, 3, &[bear1, giant], 2);
    g.pass();
    let ip = g.card(1, "Invert Polarity", Zone::Hand);
    g.choose(Choice::Cast(ip));
    let t = spell(&g, fc);
    quiet_until_with(&mut g, at_priority, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    g.pass();
    g.force_chance(&[key(KEY_COIN, 1, 1)]);
    g.pass();
    let asked = Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::NewTarget) => {
            asked.set(asked.get() + 1);
            assert!(g.has_option(Choice::Pass));
            if asked.get() == 1 {
                assert!(!g.has_option(g.tgt(giant)), "the other target of the same word: {}", opts(g));
                Some(g.tgt(bear0))
            } else {
                Some(Choice::Pass)
            }
        }
        _ => None,
    });
    assert_eq!(asked.get(), 2, "one new-target decision per target\n{}", log(&g));
    assert_eq!(g.obj(bear0).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(bear1).damage, 0);
    assert_eq!(g.obj(giant).damage, 1);
    assert_eq!(g.life(0), 17, "P0 paid the X life");
}

// ---- copies of modal, spree, X and divided spells: Lutri (CR 707.10) --------------------------

/// P0 casts Lutri with `spell` on the stack; its enter trigger targets
/// `spell`; `answer` answers the copy's decisions (else first options).
fn lutri_copies(g: &mut Game, spell: CardIdx, answer: impl Fn(&Game) -> Option<Choice>) {
    let l = g.card(0, LUTRI, Zone::Hand);
    g.choose(Choice::Cast(l));
    let t = Choice::Target(Target::Obj(g.obj_ref(spell)));
    let targeted = Cell::new(false);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) if !targeted.get() && g.obj(l).zone == Zone::Battlefield && g.has_option(t) => {
            targeted.set(true);
            Some(t)
        }
        _ => answer(g),
    });
    assert!(targeted.get(), "Lutri's trigger targeted the spell\n{}", log(g));
}

#[test]
fn lutris_copy_of_a_modal_spell_has_the_same_modes_cr_707_10() {
    // ruling: 23cf3168
    // Fiery Confluence, "2 damage to each opponent" three times: the copy
    // has the same modes (no mode is chosen for it): 12 damage.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 4])
        .battlefield(0, &["Island"; 3])
        .hand(0, &["Fiery Confluence", LUTRI])
        .start();
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    for _ in 0..3 {
        assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "{}", opts(&g));
        g.choose(Choice::Number(1));
    }
    cast_done(&mut g);
    let modes = g.stack()[0].modes.clone();
    let asked = Cell::new(false);
    lutri_copies(&mut g, fc, |g| {
        if g.kind() == Some(DecisionKind::ChooseMode) {
            asked.set(true);
        }
        None
    });
    assert!(!asked.get(), "no mode chosen for the copy");
    assert_eq!(modes.len(), 3);
    assert_eq!(g.life(1), 8, "{}", log(&g));
}

#[test]
fn mystic_confluences_copy_may_get_new_targets_not_new_modes_cr_707_10c() {
    // ruling: 3ef23eda
    // Mystic Confluence: return P1's Bear, draw, draw. Lutri's copy: the
    // same modes, a new target (P1's Reacher): both returned, four cards.
    let mut g = scen()
        .battlefield(0, &["Island"; 8])
        .battlefield(1, &["Test Bear", "Test Reacher"])
        .hand(0, &["Mystic Confluence", LUTRI])
        .start();
    let (bear, reacher) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Reacher"));
    let mc = g.card(0, "Mystic Confluence", Zone::Hand);
    g.choose(Choice::Cast(mc));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(2));
    g.choose(Choice::Number(2));
    g.choose(g.tgt(bear));
    cast_done(&mut g);
    let hand = g.player(0).hand.len(); // Lutri
    let asked = Cell::new(false);
    lutri_copies(&mut g, mc, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            asked.set(true);
            None
        }
        Some(DecisionKind::NewTarget) => Some(g.tgt(reacher)),
        _ => None,
    });
    assert!(!asked.get());
    assert_eq!(g.obj(bear).zone, Zone::Hand);
    assert_eq!(g.obj(reacher).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.player(0).hand.len(), hand - 1 + 4, "Lutri cast, four cards drawn");
}

#[test]
fn three_steps_ahead_copied_keeps_its_modes_and_may_target_anew_cr_702_172_707_10c() {
    // ruling: d4f44dfc
    // Three Steps Ahead with "counter target spell" (P0's own Bolt) and
    // "draw two, then discard": Lutri's copy has the same two modes and its
    // counter retargeted at the original Three Steps Ahead, which it
    // counters: the Bolt resolves, and P0 drew two once.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 3])
        .battlefield(0, &["Island"; 6])
        .hand(0, &["Lightning Bolt", "Three Steps Ahead", LUTRI])
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(1));
    cast_done(&mut g);
    let tsa = g.card(0, "Three Steps Ahead", Zone::Hand);
    g.choose(Choice::Cast(tsa));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(0));
    g.choose(Choice::Number(2));
    if g.kind() == Some(DecisionKind::ChooseMode) {
        g.choose(Choice::Pass);
    }
    let tb = spell(&g, bolt);
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(tb);
    cast_done(&mut g);
    let original = g.stack().last().unwrap().modes.clone();
    let hand = g.player(0).hand.len(); // Lutri
    let asked = Cell::new(false);
    let to_tsa = spell(&g, tsa);
    lutri_copies(&mut g, tsa, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            asked.set(true);
            None
        }
        Some(DecisionKind::NewTarget) => Some(to_tsa),
        _ => None,
    });
    assert!(!asked.get());
    assert_eq!(original.len(), 2);
    assert_eq!(g.obj(tsa).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 17, "the Bolt wasn't countered\n{}", log(&g));
    assert_eq!(g.player(0).hand.len(), hand - 1 + 2 - 1, "Lutri cast; drew two, discarded one, once");
}

#[test]
fn lutris_copy_keeps_x_the_division_and_the_cost_paid_new_targets_allowed_cr_707_10_601_2d() {
    // ruling: 8e0d3377 cf220ddc 2213efb8 7be1b615
    // Fire Covenant, X = 3 (3 life paid once): 2 to the Bear, 1 to the
    // Giant. The copy: the first target changed to the Wall, the second
    // kept; X and the division copied: the Wall is dealt 2, the Giant 1 + 1.
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain", "Mountain", "Island", "Island", "Island"])
        .battlefield(1, &["Test Bear", "Test Giant", "Test Wall"])
        .hand(0, &["Fire Covenant", LUTRI])
        .start();
    let (bear, giant, wall) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Test Giant"), bf(&g, 1, "Test Wall"));
    let fc = fire_covenant(&mut g, 3, &[bear, giant], 2);
    let first = Cell::new(true);
    lutri_copies(&mut g, fc, |g| match g.kind() {
        Some(DecisionKind::NewTarget) if first.get() => {
            first.set(false);
            Some(g.tgt(wall))
        }
        Some(DecisionKind::NewTarget) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(wall).damage, 2, "{}", log(&g));
    assert_eq!(g.obj(giant).damage, 2);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.life(0), 17, "the copy pays no life");
}

#[test]
fn lutri_copies_a_spell_without_targets_cr_707_10() {
    // ruling: b7a14c0f
    // Brainstorm (no target), copied: P0 draws three and puts back two
    // twice.
    let mut g = scen().battlefield(0, &["Island"; 4]).hand(0, &["Brainstorm", LUTRI]).library(0, &["Island"; 12]).start();
    let b = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(b));
    cast_done(&mut g);
    let lib = g.player(0).library.len();
    lutri_copies(&mut g, b, |_| None);
    assert_eq!(g.player(0).library.len(), lib - 2, "{}", log(&g));
}
