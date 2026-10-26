//! M8d part 2 (ENGINE.md 38): rulings about spells cast from other zones and
//! without paying their mana cost (37.7's list): Etali, Primal Conqueror's
//! free casts during its trigger's resolution (608.2g, 601.2, 107.3b,
//! 118.9a, 601.2f), Bolas's Citadel's casts and land plays from the top of
//! the library (118.9, 305.2, 307.1), Dauthi Voidwalker and a discarded card
//! (614.1a, 701.9), converge counting no colours for a copy or a free cast
//! (Prismatic Ending, 707.10, 601.2h). `ruling: <hash8>` names the ruling.

mod common;
mod rulings_util;

use engine::db::AbilitySlot;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};
use rulings_util::*;
use std::cell::{Cell, RefCell};

const ETALI: &str = "Etali, Primal Conqueror // Etali, Primal Sickness";
const LUTRI: &str = "Lutri, the Spellchaser";

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn act(g: &Game, c: CardIdx) -> Choice {
    (0..8u8)
        .map(|i| Choice::Ability(c, AbilitySlot::Activated(i)))
        .find(|&a| g.has_option(a))
        .unwrap_or_else(|| panic!("no ability of {} offered: {}", g.name(c), opts(g)))
}

fn exiled_offered(g: &Game) -> Vec<String> {
    g.pending_decision()
        .unwrap()
        .options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Card(c) => Some(g.name(c).to_string()),
            _ => None,
        })
        .collect()
}

fn pick(g: &Game, name: &str) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| matches!(c, Choice::Card(x) if g.name(x) == name))
}

/// Cast Etali (seven Mountains) and stop at its first free-cast choice.
fn etali_until_choice(g: &mut Game) {
    let e = g.card(0, ETALI, Zone::Hand);
    g.choose(Choice::Cast(e));
    quiet_until(g, |g| g.kind() == Some(DecisionKind::ChooseExiled) || (g.stack().is_empty() && at_priority(g)));
}

// ---- Etali, Primal Conqueror ------------------------------------------------------------------

#[test]
fn etali_a_later_spell_may_target_an_earlier_one_cr_608_2g() {
    // ruling: 45dd8bdb
    // "You may cast any number of spells from among the nonland cards
    // exiled this way without paying their mana costs": P0's Bolt (at P1),
    // then P1's Counterspell, which can target the Bolt on the stack.
    let mut g = scen()
        .library(0, &["Lightning Bolt", "Island"])
        .library(1, &["Counterspell", "Forest"])
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &[ETALI])
        .start();
    etali_until_choice(&mut g);
    let bolt = g.card(0, "Lightning Bolt", Zone::Exile);
    g.choose(Choice::Card(bolt));
    g.choose(player(1));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::ChooseExiled));
    let cs = g.card(1, "Counterspell", Zone::Exile);
    g.choose(Choice::Card(cs));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    let tb = Choice::Target(Target::Obj(g.obj_ref(bolt)));
    assert!(g.has_option(tb), "the Bolt, a spell on the stack: {}", opts(&g));
    g.choose(tb);
    resolve(&mut g);
    assert_eq!(g.life(1), 20, "the Bolt was countered\n{}", log(&g));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
}

#[test]
fn etali_a_later_spell_cant_target_the_permanent_an_earlier_one_will_become_cr_608_2g() {
    // ruling: 45dd8bdb dbec9eeb
    // The Bear first (a creature spell on the stack, not yet a creature),
    // then Swords to Plowshares: only Etali is a creature to target. The
    // Bear's spell on the stack doesn't stop the sorcery Faithless Looting
    // from being cast in the same resolution (timing ignored).
    let mut g = scen()
        .library(0, &["Test Bear", "Island"])
        .library(1, &["Swords to Plowshares", "Forest"])
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &[ETALI])
        .start();
    etali_until_choice(&mut g);
    let e = g.card(0, ETALI, Zone::Battlefield);
    let bear = g.card(0, "Test Bear", Zone::Exile);
    g.choose(Choice::Card(bear));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::ChooseExiled));
    let sw = g.card(1, "Swords to Plowshares", Zone::Exile);
    g.choose(Choice::Card(sw));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(bear)), "{}", opts(&g));
    assert!(g.has_option(g.tgt(e)));
}

#[test]
fn etali_casts_a_sorcery_with_a_spell_on_the_stack_during_its_resolution_cr_608_2g() {
    // ruling: dbec9eeb
    let mut g = scen()
        .library(0, &["Test Bear", "Island"])
        .library(1, &["Faithless Looting", "Forest", "Forest", "Forest"])
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &[ETALI])
        .start();
    etali_until_choice(&mut g);
    let bear = g.card(0, "Test Bear", Zone::Exile);
    g.choose(Choice::Card(bear));
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::ChooseExiled));
    assert!(g.stack().iter().any(|s| matches!(s.kind, engine::game::StackKind::Spell { card } if card == bear)));
    let fl = g.card(1, "Faithless Looting", Zone::Exile);
    assert!(g.has_option(Choice::Card(fl)), "a sorcery, timing ignored: {}", opts(&g));
    g.choose(Choice::Card(fl));
    resolve(&mut g);
    assert_eq!(g.obj(fl).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
}

#[test]
fn etali_a_free_x_spell_has_x_zero_cr_107_3b() {
    // ruling: 8f234701
    let mut g = scen()
        .library(0, &["Walking Ballista", "Island"])
        .library(1, &["Forest", "Test Bear"])
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &[ETALI])
        .start();
    etali_until_choice(&mut g);
    let wb = g.card(0, "Walking Ballista", Zone::Exile);
    let asked_x = Cell::new(false);
    g.choose(Choice::Card(wb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseExiled) => Some(Choice::Pass),
        Some(DecisionKind::ChooseMode) => {
            asked_x.set(g.pending_decision().unwrap().options.iter().any(|o| o.choice != Choice::Number(0)));
            None
        }
        _ => None,
    });
    assert!(!asked_x.get(), "X can only be 0");
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "a 0/0 that dies\n{}", log(&g));
}

#[test]
fn etali_no_alternative_cost_but_mandatory_additional_costs_cr_118_9a_601_2f() {
    // ruling: a2b92065
    // Abhorrent Oculus ("As an additional cost to cast this spell, exile six
    // cards from your graveyard"): offered with six cards in P0's graveyard,
    // not with five. Solitude is cast free (no choice of its evoke cost: it
    // stays on the battlefield).
    for (gy, castable) in [(5usize, false), (6, true)] {
        let mut g = scen()
            .library(0, &["Abhorrent Oculus", "Island"])
            .library(1, &["Solitude", "Forest"])
            .battlefield(0, &["Mountain"; 7])
            .graveyard(0, &vec!["Island"; gy])
            .hand(0, &[ETALI])
            .start();
        etali_until_choice(&mut g);
        let names = exiled_offered(&g);
        assert_eq!(names.iter().any(|n| n == "Abhorrent Oculus"), castable, "{gy} cards: {names:?}");
        assert!(names.iter().any(|n| n == "Solitude"), "{names:?}");
        let alt = Cell::new(false);
        let picks = RefCell::new(vec!["Solitude", "Abhorrent Oculus"]);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseExiled) => {
                let mut p = picks.borrow_mut();
                while let Some(n) = p.first().copied() {
                    p.remove(0);
                    if let Some(c) = pick(g, n) {
                        return Some(c);
                    }
                }
                Some(Choice::Pass)
            }
            Some(DecisionKind::ChooseAltCost) => {
                alt.set(true);
                None
            }
            Some(DecisionKind::Target) if g.has_option(Choice::Pass) => Some(Choice::Pass),
            _ => None,
        });
        assert!(!alt.get(), "no alternative cost offered");
        assert!(g.battlefield().iter().any(|&c| g.name(c) == "Solitude" && g.obj(c).controller == 0), "not evoked
{}", log(&g));
        if castable {
            assert_eq!(g.count_in(0, "Abhorrent Oculus", Zone::Battlefield), 1, "{}", log(&g));
            assert_eq!(g.player(0).graveyard.len(), 0, "six exiled");
        }
    }
}

#[test]
fn etali_cards_not_cast_stay_in_exile_lands_too_cr_406() {
    // ruling: 8b4092d0
    let mut g = scen()
        .library(0, &["Island", "Lightning Bolt", "Island"])
        .library(1, &["Test Bear", "Forest"])
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &[ETALI])
        .start();
    etali_until_choice(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseExiled)).then_some(Choice::Pass));
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2);
    assert_eq!(g.count_in(0, "Island", Zone::Exile), 1);
    assert_eq!(g.count_in(0, "Lightning Bolt", Zone::Exile), 1);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile), 1);
    let bolt = g.card(0, "Lightning Bolt", Zone::Exile);
    assert!(!g.has_option(Choice::Cast(bolt)));
}

// ---- Bolas's Citadel ----------------------------------------------------------------------

fn top(g: &Game, p: u8) -> CardIdx {
    *g.player(p).library.last().unwrap()
}

#[test]
fn citadel_an_x_spell_from_the_top_has_x_zero_and_costs_no_life_cr_107_3b() {
    // ruling: 3c753455
    let mut g = scen()
        .library(0, &["Walking Ballista", "Island"])
        .battlefield(0, &["Bolas's Citadel", "Island", "Island"])
        .start();
    let wb = top(&g, 0);
    g.choose(Choice::Cast(wb));
    let asked_x = Cell::new(false);
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseMode) {
            asked_x.set(g.pending_decision().unwrap().options.iter().any(|o| o.choice != Choice::Number(0)));
        }
        None
    });
    assert!(!asked_x.get(), "X can only be 0");
    assert_eq!(g.life(0), 20, "life equal to its mana value, 0");
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "{}", log(&g));
}

#[test]
fn citadel_no_other_alternative_cost_but_additional_costs_cr_118_9a_601_2f() {
    // ruling: 4c7c58fa
    // Force of Will on top: 5 life, never its own alternative cost.
    let mut g = scen()
        .library(0, &["Force of Will", "Island"])
        .battlefield(0, &["Bolas's Citadel"])
        .battlefield(1, &["Mountain"])
        .hand(0, &["Brainstorm"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(0));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let fow = top(&g, 0);
    assert!(g.has_option(Choice::Cast(fow)), "an instant on the opponent's turn: {}", opts(&g));
    g.choose(Choice::Cast(fow));
    let alt = Cell::new(false);
    let tb = Choice::Target(Target::Obj(g.obj_ref(bolt)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => {
            alt.set(true);
            None
        }
        Some(DecisionKind::Target) => Some(tb),
        _ => None,
    });
    assert!(!alt.get());
    assert_eq!(g.life(0), 15, "5 life, the Bolt countered\n{}", log(&g));
    assert_eq!(g.count_in(0, "Brainstorm", Zone::Hand), 1, "no blue card exiled");
    // Abhorrent Oculus on top: castable only with six cards to exile.
    for (gy, castable) in [(5usize, false), (6, true)] {
        let g = scen()
            .library(0, &["Abhorrent Oculus", "Island"])
            .battlefield(0, &["Bolas's Citadel"])
            .graveyard(0, &vec!["Island"; gy])
            .start();
        let oc = top(&g, 0);
        assert_eq!(g.has_option(Choice::Cast(oc)), castable, "{gy}: {}", opts(&g));
    }
}

#[test]
fn citadel_a_land_from_the_top_needs_a_land_drop_cr_305_2() {
    // ruling: 8ccdbbbd
    for (played, playable) in [(0u8, true), (1, false)] {
        let g = scen().library(0, &["Forest", "Island"]).battlefield(0, &["Bolas's Citadel"]).lands_played(0, played).start();
        let f = top(&g, 0);
        assert_eq!(g.has_option(Choice::Play(f)), playable, "{played} played: {}", opts(&g));
    }
}

#[test]
fn citadel_spells_from_the_top_follow_their_timing_cr_307_1() {
    // ruling: 8f6f883e
    // On P1's turn: a sorcery on top isn't offered; on P0's own turn with a
    // spell on the stack neither.
    let g = scen().library(0, &["Faithless Looting", "Island"]).battlefield(0, &["Bolas's Citadel"]).battlefield(1, &["Mountain"]).hand(1, &["Lightning Bolt"]).at(Step::Main1, 1).start();
    let mut g = g;
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(0));
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let fl = top(&g, 0);
    assert!(!g.has_option(Choice::Cast(fl)), "{}", opts(&g));
    let mut g = scen()
        .library(0, &["Faithless Looting", "Island"])
        .battlefield(0, &["Bolas's Citadel", "Island"])
        .hand(0, &["Brainstorm"])
        .start();
    let fl = top(&g, 0);
    assert!(g.has_option(Choice::Cast(fl)), "{}", opts(&g));
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(at_priority);
    assert_eq!(g.stack().len(), 1);
    assert!(!g.has_option(Choice::Cast(fl)), "a sorcery with a spell on the stack: {}", opts(&g));
}

// ---- Dauthi Voidwalker and a discarded card ---------------------------------------------------

#[test]
fn dauthi_a_discard_exiled_instead_still_triggers_discard_abilities_cr_614_1a() {
    // ruling: 8d9db447
    // P1's Dauthi Voidwalker; P0 controls Inti, Seneschal of the Sun
    // ("Whenever you discard one or more cards, exile the top card of your
    // library. You may play that card until your next end step.") and
    // casts Faithless Looting: the two discarded cards are exiled with void
    // counters, and Inti's ability still triggers.
    let mut g = scen()
        .library(0, &["Island", "Island", "Forest", "Island"])
        .battlefield(0, &["Inti, Seneschal of the Sun", "Mountain"])
        .battlefield(1, &["Dauthi Voidwalker"])
        .hand(0, &["Faithless Looting"])
        .start();
    let fl = g.card(0, "Faithless Looting", Zone::Hand);
    g.choose(Choice::Cast(fl));
    resolve(&mut g);
    let voided: Vec<CardIdx> = g.exile_zone().iter().copied().filter(|&c| g.obj(c).owner == 0 && !g.counters_of(c).is_empty()).collect();
    assert_eq!(voided.len(), 3, "the two discarded cards and Faithless Looting, exiled with void counters
{}", log(&g));
    assert!(g.obj(fl).zone == Zone::Exile);
    let forest = g.card(0, "Forest", Zone::Exile);
    assert!(g.counters_of(forest).is_empty(), "Inti's exile: the top card\n{}", log(&g));
}

#[test]
fn dauthi_occult_epiphany_counts_the_types_of_cards_discarded_into_exile_cr_614_1a() {
    // ruling: 8d9db447
    // "Draw X cards, then discard X cards. Create a 1/1 white Spirit ... for
    // each card type among cards discarded this way": the cards are exiled
    // by P1's Dauthi but still were discarded (creature, instant, land).
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .battlefield(1, &["Dauthi Voidwalker"])
        .hand(0, &["Occult Epiphany"])
        .library(0, &["Test Bear", "Lightning Bolt", "Forest", "Island"])
        .start();
    let oe = g.card(0, "Occult Epiphany", Zone::Hand);
    g.choose(Choice::Cast(oe));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(3));
    resolve(&mut g);
    assert!(g.player(0).hand.is_empty());
    assert_eq!(g.count_in(0, "Test Bear", Zone::Exile), 1);
    assert_eq!(tokens_named(&g, "Spirit").len(), 3, "{}", log(&g));
}

// ---- converge with no mana spent (Prismatic Ending) -------------------------------------------

#[test]
fn prismatic_ending_copied_counts_no_colours_cr_707_10_601_2h() {
    // ruling: 41553eae
    // X = 0 paid with {W}: converge 1 exiles P1's Sol Ring (mana value 1).
    // Lutri's copy, retargeted at P1's Mana Vault (mana value 1): no mana
    // was spent to cast the copy, converge 0: not exiled.
    let mut g = scen()
        .battlefield(0, &["Plains", "Island", "Island", "Island"])
        .battlefield(1, &["Sol Ring", "Mana Vault"])
        .hand(0, &["Prismatic Ending", LUTRI])
        .start();
    let (ring, vault) = (bf(&g, 1, "Sol Ring"), bf(&g, 1, "Mana Vault"));
    let pe = g.card(0, "Prismatic Ending", Zone::Hand);
    g.choose(Choice::Cast(pe));
    g.drive_until(|g| !at_priority(g));
    if g.kind() == Some(DecisionKind::ChooseMode) {
        g.choose(Choice::Number(0));
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(ring));
    quiet_until(&mut g, at_priority);
    let l = g.card(0, LUTRI, Zone::Hand);
    g.choose(Choice::Cast(l));
    let t = Choice::Target(Target::Obj(g.obj_ref(pe)));
    let tv = g.tgt(vault);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::NewTarget) => Some(tv),
        _ => None,
    });
    assert_eq!(g.obj(ring).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.obj(vault).zone, Zone::Battlefield, "the copy's converge is 0\n{}", log(&g));
}

/// P0 Thoughtseizes `card` out of P1's hand (voided by P0's Dauthi), then
/// activates the Dauthi choosing it.
fn void_and_choose(g: &mut Game, card: &str) -> CardIdx {
    let ts = g.card(0, "Thoughtseize", Zone::Hand);
    let target = g.card(1, card, Zone::Hand);
    g.choose(Choice::Cast(ts));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseHandCard) | Some(DecisionKind::ChooseDiscard) | Some(DecisionKind::Discard) => {
            g.has_option(Choice::Card(target)).then_some(Choice::Card(target))
        }
        _ => None,
    });
    assert_eq!(g.obj(target).zone, Zone::Exile);
    let d = bf(g, 0, "Dauthi Voidwalker");
    g.choose(act(g, d));
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::ChooseExiled)).then_some(Choice::Card(target)));
    target
}

#[test]
fn prismatic_ending_cast_free_counts_no_colours_cr_601_2h() {
    // ruling: fc8abf83
    // Cast with Dauthi Voidwalker without paying its mana cost: X = 0 and
    // converge 0: a Mox (mana value 0) is exiled, Sol Ring (1) isn't.
    for (target, exiled) in [("Sol Ring", false), ("Mox Pearl", true)] {
        let mut g = scen()
            .battlefield(0, &["Dauthi Voidwalker", "Swamp"])
            .battlefield(1, &[target])
            .hand(0, &["Thoughtseize"])
            .hand(1, &["Prismatic Ending"])
            .start();
        let pe = void_and_choose(&mut g, "Prismatic Ending");
        let t = bf(&g, 1, target);
        assert!(g.has_option(Choice::Cast(pe)), "{}", opts(&g));
        g.choose(Choice::Cast(pe));
        let tt = g.tgt(t);
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(tt));
        assert_eq!(g.obj(t).zone == Zone::Exile, exiled, "{target}\n{}", log(&g));
    }
}
