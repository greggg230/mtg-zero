//! M8d part 3 (ENGINE.md 39): the cube's stack rulings left after the
//! triage was narrowed: a spell or ability whose targets are all gone does
//! nothing at all, one with a target left does what it can (CR 608.2b);
//! resolutions and casts that no player can interrupt (608.2, 601.2,
//! 117.3); who gets priority first after a resolution (117.3b); timing of
//! spells cast during a resolution (608.2g) or from other zones; copies on
//! the stack (707.10); splice (702.47); uncounterable spells and effects
//! beyond the counter (101.2); special actions (116.2). `ruling: <hash8>`
//! names the ruling (docs/rulings.md).

mod common;
mod rulings_util;

use engine::db::{AbilitySlot, Counter};
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use rulings_util::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn ab(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn pay(g: &mut Game) {
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::PayPool)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
}

/// Answer decisions with `answer` (else first options) until P0 has
/// priority with something on the stack.
fn until_p0_priority(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    quiet_until_with(g, |g| at_priority(g) && !g.stack().is_empty() && g.turn().priority == 0, |g| match g.kind() {
        Some(DecisionKind::Priority) => Some(Choice::Pass),
        _ => answer(g).or_else(|| Some(g.pending_decision().unwrap().options[0].choice)),
    });
}

/// P0, holding priority, Bolts `kill` and everything resolves (`answer`
/// for decisions on the way).
fn bolt_then_resolve(g: &mut Game, kill: CardIdx, answer: impl Fn(&Game) -> Option<Choice>) {
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    let t = g.tgt(kill);
    let done = std::cell::Cell::new(false);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) if !done.get() && g.has_option(t) => {
            done.set(true);
            Some(t)
        }
        _ => answer(g),
    });
}

fn target_of(t: CardIdx) -> impl Fn(&Game) -> Option<Choice> {
    move |g: &Game| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(t))).then(|| g.tgt(t))
}

// ---- every target gone: nothing happens (CR 608.2b) --------------------------------------------

#[test]
fn removal_whose_target_is_gone_does_none_of_its_other_effects_cr_608_2b() {
    // ruling: d0a321ea b3258d86 74a7cb90 546924df 09936adc 41ffaf2b ec37be99
    // Each spell targets P1's Bear; P0, holding priority, Bolts the Bear
    // first. Galvanic Discharge: no energy got or paid. Erode and Path to
    // Exile: P1 searches nothing. Oust: P1 gains no life. Traumatic
    // Critique: P0 neither draws nor discards. Chain Lightning: no copy.
    let setup = |spell: &str| {
        scen()
            .battlefield(0, &["Mountain", "Mountain", "Mountain", "Plains", "Island", "Island", "Island"])
            .hand(0, &[spell, "Lightning Bolt"])
            .battlefield(1, &["Test Bear", "Mountain"])
            .library(1, &["Plains", "Forest", "Island"])
            .at(Step::Main1, 0)
            .start()
    };
    for spell in ["Galvanic Discharge", "Erode", "Path to Exile", "Oust", "Traumatic Critique", "Chain Lightning"] {
        let mut g = setup(spell);
        let bear = bf(&g, 1, "Test Bear");
        let (life1, lib1, hand0) = (g.life(1), g.player(1).library.len(), g.player(0).hand.len());
        let c = g.card(0, spell, Zone::Hand);
        g.choose(Choice::Cast(c));
        until_p0_priority(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
            _ => target_of(bear)(g),
        });
        bolt_then_resolve(&mut g, bear, |g| match g.kind() {
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            Some(DecisionKind::ChooseMode) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).max_by_key(|c| match c {
                Choice::Number(n) => *n as i32,
                _ => -1,
            }),
            _ => None,
        });
        assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{spell}");
        assert_eq!(g.player(0).energy, 0, "{spell}: no energy\n{}", log(&g));
        assert_eq!(g.player(1).library.len(), lib1, "{spell}: no search\n{}", log(&g));
        assert_eq!(g.life(1), life1, "{spell}: no life gained or lost\n{}", log(&g));
        assert_eq!(g.player(0).hand.len(), hand0 - 2, "{spell}: no draw, no discard\n{}", log(&g));
        assert_eq!(g.obj(c).zone, Zone::Graveyard, "{spell}");
    }
}

#[test]
fn abilities_whose_target_is_gone_do_none_of_their_other_effects_cr_608_2b() {
    // ruling: f0ca93ac a6497c7f c90ba9cf 22120a2e
    // Teferi's -3 at P1's Bear: no draw. Orcish Bowmasters's ping at it: no
    // Army. Arwen's ability at it (her counter removed as the cost): no
    // counters on Arwen. Oko's exchange of P0's Food and P1's Bear: no
    // exchange. P0 Bolts the Bear in response each time.
    let base = || {
        scen()
            .battlefield(0, &["Mountain", "Mountain", "Swamp", "Swamp", "Island"])
            .hand(0, &["Lightning Bolt"])
            .battlefield(1, &["Test Bear"])
            .at(Step::Main1, 0)
    };
    // Teferi.
    let mut g = base().battlefield(0, &["Teferi, Time Raveler"]).start();
    let bear = bf(&g, 1, "Test Bear");
    let tef = bf(&g, 0, "Teferi, Time Raveler");
    let hand = g.player(0).hand.len();
    g.choose(ab(tef, 1));
    until_p0_priority(&mut g, target_of(bear));
    bolt_then_resolve(&mut g, bear, |_| None);
    assert_eq!(g.player(0).hand.len(), hand - 1, "Teferi: no draw\n{}", log(&g));
    // Bowmasters.
    let mut g = base().hand(0, &["Orcish Bowmasters"]).start();
    let bear = bf(&g, 1, "Test Bear");
    let ob = g.card(0, "Orcish Bowmasters", Zone::Hand);
    g.choose(Choice::Cast(ob));
    pay(&mut g);
    g.drive_until(|g| g.obj(ob).zone == Zone::Battlefield);
    until_p0_priority(&mut g, target_of(bear));
    bolt_then_resolve(&mut g, bear, |_| None);
    assert!(tokens_named(&g, "Orc Army").is_empty(), "Bowmasters: no amass\n{}", log(&g));
    // Arwen.
    let indestructible = Counter::Kw(engine::db::Kw::Indestructible);
    let mut g = base().battlefield(0, &["Arwen, Mortal Queen"]).counters(0, "Arwen, Mortal Queen", indestructible, 1).start();
    let bear = bf(&g, 1, "Test Bear");
    let ar = bf(&g, 0, "Arwen, Mortal Queen");
    g.choose(ab(ar, 0));
    until_p0_priority(&mut g, |g| {
        let mine = bf(g, 1, "Test Bear");
        target_of(mine)(g)
    });
    bolt_then_resolve(&mut g, bear, |_| None);
    assert_eq!(g.counters_on(ar, Counter::P1P1), 0, "Arwen: no counter\n{}", log(&g));
    // Oko's exchange.
    let mut g = base().battlefield(0, &["Oko, Thief of Crowns"]).counters(0, "Oko, Thief of Crowns", Counter::Loyalty, 8).start();
    let oko = bf(&g, 0, "Oko, Thief of Crowns");
    g.choose(ab(oko, 0));
    resolve(&mut g);
    let food = tokens_named(&g, "Food")[0];
    let bear = bf(&g, 1, "Test Bear");
    let t = std::cell::Cell::new(0);
    // (A second activation the same turn isn't allowed: next turn.)
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().number > 3 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(ab(oko, 2));
    until_p0_priority(&mut g, |g| {
        if g.kind() != Some(DecisionKind::Target) {
            return None;
        }
        t.set(t.get() + 1);
        if g.has_option(g.tgt(food)) {
            Some(g.tgt(food))
        } else {
            Some(g.tgt(bear))
        }
    });
    bolt_then_resolve(&mut g, bear, |_| None);
    assert_eq!(g.obj(food).controller, 0, "Oko: no exchange\n{}", log(&g));
}

#[test]
fn modal_spells_with_every_target_gone_do_nothing_and_with_one_left_do_the_rest_cr_608_2b() {
    // ruling: 9dcd9c8e acbc3ade 61ed8f24 d6bf1964 61f2c92b 8d406a15
    // Three Steps Ahead countering P1's Bolt and drawing two: P0 counters
    // the Bolt first (Counterspell): no draw. Fiery Confluence destroying
    // P1's Sol Ring once and dealing 2 to each opponent twice: P0 Abrades
    // the Sol Ring first: no damage. Kolaghan's Command destroying the Sol
    // Ring and dealing 2 to the Bear, the Bear Bolted: the Sol Ring is
    // destroyed. Collective Brutality giving the Bear -2/-2 and draining P1,
    // the Bear Bolted: the drain happens. Pest Infestation (X = 1) at the Sol
    // Ring, Abraded: no Pests.
    // Three Steps Ahead.
    let mut g = scen()
        .battlefield(0, &["Island"; 8])
        .hand(0, &["Three Steps Ahead", "Counterspell"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(0));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let tsa = g.card(0, "Three Steps Ahead", Zone::Hand);
    let hand = g.player(0).hand.len();
    g.choose(Choice::Cast(tsa));
    let picked = std::cell::Cell::new(0);
    until_p0_priority(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            let c = [Choice::Number(0), Choice::Number(2), Choice::Pass][picked.get().min(2)];
            picked.set(picked.get() + 1);
            g.has_option(c).then_some(c).or(Some(Choice::Pass))
        }
        Some(DecisionKind::Target) => Some(g.tgt(b)),
        _ => None,
    });
    let cs = g.card(0, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(b);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(b).zone, Zone::Graveyard);
    assert_eq!(g.player(0).hand.len(), hand - 2, "Three Steps Ahead did nothing: no draw\n{}", log(&g));
    // Fiery Confluence: Sol Ring once, 2 to each opponent twice.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .hand(0, &["Fiery Confluence", "Abrade"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    let modes = std::cell::Cell::new(0);
    until_p0_priority(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            modes.set(modes.get() + 1);
            // (Chosen in non-decreasing order: 2 to each opponent twice,
            // then the Sol Ring.)
            Some(Choice::Number(if modes.get() == 3 { 2 } else { 1 }))
        }
        Some(DecisionKind::Target) => Some(g.tgt(ring)),
        _ => None,
    });
    let ab_ = g.card(0, "Abrade", Zone::Hand);
    g.choose(Choice::Cast(ab_));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(ring)),
        _ => None,
    });
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 20, "every target illegal: no damage either\n{}", log(&g));
    // Kolaghan's Command and Collective Brutality: one target left.
    for spell in ["Kolaghan's Command", "Collective Brutality"] {
        let mut g = scen()
            .battlefield(0, &["Mountain", "Mountain", "Mountain", "Swamp", "Swamp"])
            .hand(0, &[spell, "Lightning Bolt", "Island"])
            .battlefield(1, &["Sol Ring", "Test Bear"])
            .at(Step::Main1, 0)
            .start();
        let ring = bf(&g, 1, "Sol Ring");
        let bear = bf(&g, 1, "Test Bear");
        let c = g.card(0, spell, Zone::Hand);
        g.choose(Choice::Cast(c));
        let modes = std::cell::Cell::new(0);
        let kc = spell == "Kolaghan's Command";
        until_p0_priority(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => {
                modes.set(modes.get() + 1);
                // Kolaghan's: destroy artifact (2), 2 damage (3). Brutality:
                // -2/-2 (1), drain (2), then done.
                let want = if kc { [2u8, 3][(modes.get() - 1).min(1)] } else { [1u8, 2][(modes.get() - 1).min(1)] };
                let c = Choice::Number(want);
                if modes.get() > 2 || !g.has_option(c) {
                    Some(Choice::Pass)
                } else {
                    Some(c)
                }
            }
            Some(DecisionKind::Target) if g.has_option(g.tgt(ring)) => Some(g.tgt(ring)),
            Some(DecisionKind::Target) if g.has_option(g.tgt(bear)) => Some(g.tgt(bear)),
            Some(DecisionKind::Target) => Some(player(1)),
            Some(DecisionKind::DiscardCost) | Some(DecisionKind::ChooseCost) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
            _ => None,
        });
        bolt_then_resolve(&mut g, bear, |_| None);
        if kc {
            assert_eq!(g.obj(ring).zone, Zone::Graveyard, "{spell}: the other target\n{}", log(&g));
        } else {
            assert_eq!((g.life(0), g.life(1)), (22, 18), "{spell}: the drain\n{}", log(&g));
        }
    }
    // Pest Infestation at the Sol Ring, Abraded.
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Mountain", "Mountain"])
        .hand(0, &["Pest Infestation", "Abrade"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    let pi = g.card(0, "Pest Infestation", Zone::Hand);
    g.choose(Choice::Cast(pi));
    until_p0_priority(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) if g.has_option(g.tgt(ring)) => Some(g.tgt(ring)),
        Some(DecisionKind::Target) => Some(Choice::Pass),
        _ => None,
    });
    let ab_ = g.card(0, "Abrade", Zone::Hand);
    g.choose(Choice::Cast(ab_));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(ring)),
        _ => None,
    });
    assert!(tokens_named(&g, "Pest").is_empty(), "{}", log(&g));
}

#[test]
fn abilities_and_reflexive_effects_with_their_targets_gone_cr_608_2b() {
    // ruling: 7335701d 8c334374 4c9245c3 fd8ef0a1 717f2c88
    // Goblin Welder at P1's Sol Ring and the Mox Pearl in P1's graveyard,
    // the Sol Ring Abraded: nothing returns. Badgermole Cub's earthbend at a
    // Forest P0 then Strip Mines: no counters. Emeritus's Regrowth copy at a
    // Bolt Ghost Vacuum exiles: it fails and Emeritus stays unprepared.
    // Goryo's Vengeance at Griselbrand, exiled by Ghost Vacuum: nothing;
    // spliced onto Through the Breach, the whole spell does nothing (its
    // only target gone): the Bear stays in hand.
    // Goblin Welder.
    let mut g = scen()
        .battlefield(0, &["Goblin Welder", "Mountain", "Mountain"])
        .hand(0, &["Abrade"])
        .battlefield(1, &["Sol Ring"])
        .graveyard(1, &["Mox Pearl"])
        .at(Step::Main1, 0)
        .start();
    let gw = bf(&g, 0, "Goblin Welder");
    let ring = bf(&g, 1, "Sol Ring");
    let pearl = g.card(1, "Mox Pearl", Zone::Graveyard);
    g.choose(ab(gw, 0));
    until_p0_priority(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(g.tgt(ring)) => Some(g.tgt(ring)),
        Some(DecisionKind::Target) => Some(g.tgt(pearl)),
        _ => None,
    });
    let a = g.card(0, "Abrade", Zone::Hand);
    g.choose(Choice::Cast(a));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(ring)),
        _ => None,
    });
    assert_eq!(g.obj(pearl).zone, Zone::Graveyard, "Welder: one target illegal, nothing\n{}", log(&g));
    // Badgermole Cub.
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Strip Mine"]).hand(0, &["Badgermole Cub"]).at(Step::Main1, 0).start();
    let f = bf(&g, 0, "Forest");
    let sm = bf(&g, 0, "Strip Mine");
    let cub = g.card(0, "Badgermole Cub", Zone::Hand);
    g.choose(Choice::Cast(cub));
    pay(&mut g);
    g.drive_until(|g| g.obj(cub).zone == Zone::Battlefield);
    until_p0_priority(&mut g, target_of(f));
    let a = (0..3u8).map(|i| ab(sm, i)).find(|&a| g.has_option(a)).unwrap();
    g.choose(a);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(f))).then(|| g.tgt(f)));
    assert_eq!(g.obj(f).zone, Zone::Graveyard);
    assert!(g.battlefield().iter().all(|&c| g.counters_on(c, Counter::P1P1) == 0), "{}", log(&g));
    // Emeritus's Regrowth.
    let em = "Emeritus of Abundance // Regrowth";
    let mut g = scen().battlefield(0, &["Forest"; 5]).battlefield(0, &["Ghost Vacuum"]).graveyard(0, &["Lightning Bolt"]).hand(0, &[em]).at(Step::Main1, 0).start();
    let e = g.card(0, em, Zone::Hand);
    g.choose(Choice::Cast(e));
    resolve(&mut g);
    assert!(g.is_prepared(e));
    let copy = g.exile_zone().iter().copied().find(|&c| g.is_copy_obj(c) && g.name(c) == "Regrowth").unwrap();
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    g.choose(Choice::Cast(copy));
    until_p0_priority(&mut g, target_of(bolt));
    assert!(!g.is_prepared(e), "cast: not prepared any more");
    let gv = bf(&g, 0, "Ghost Vacuum");
    g.choose(ab(gv, 0));
    resolve_with(&mut g, target_of(bolt));
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    assert!(!g.is_prepared(e), "still unprepared\n{}", log(&g));
    // Goryo's Vengeance, alone and spliced onto Through the Breach.
    for spliced in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Badlands"; 8])
            .battlefield(0, &["Ghost Vacuum"])
            .hand(0, &["Through the Breach", "Goryo's Vengeance", "Test Bear"])
            .graveyard(0, &["Griselbrand"])
            .at(Step::Main1, 0)
            .start();
        let gris = g.card(0, "Griselbrand", Zone::Graveyard);
        let spell = g.card(0, if spliced { "Through the Breach" } else { "Goryo's Vengeance" }, Zone::Hand);
        let goryo = g.card(0, "Goryo's Vengeance", Zone::Hand);
        g.choose(Choice::Cast(spell));
        until_p0_priority(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseHandCard) if g.has_option(Choice::Card(goryo)) => Some(Choice::Card(goryo)),
            Some(DecisionKind::ChooseHandCard) => Some(Choice::Pass),
            Some(DecisionKind::Target) => Some(g.tgt(gris)),
            _ => None,
        });
        let gv = bf(&g, 0, "Ghost Vacuum");
        g.choose(ab(gv, 0));
        let bear = g.card(0, "Test Bear", Zone::Hand);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) if g.has_option(g.tgt(gris)) => Some(g.tgt(gris)),
            Some(DecisionKind::ChooseHandCard) => Some(Choice::Card(bear)),
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            _ => None,
        });
        assert_eq!(g.obj(gris).zone, Zone::Exile);
        assert_eq!(g.obj(bear).zone, Zone::Hand, "spliced {spliced}: the whole spell did nothing\n{}", log(&g));
        assert_eq!(g.obj(spell).zone, Zone::Graveyard);
    }
}

#[test]
fn triggers_with_their_targets_gone_or_condition_failed_cr_608_2b_603_4() {
    // ruling: 4404ca0c ca40ad03 7062f665
    // Fear of Missing Out's attack trigger (delirium) at the attacking Bear,
    // Bolted in response: no extra combat. Aragorn's attack trigger at P1's
    // Bear, Bolted: the whole ability does nothing, so P1's Wall may still
    // block though P0 is the monarch. Oath of Druids at P0's upkeep (P1 has
    // more creatures); P0 Bolts P1's creature: on resolution P1 no longer
    // has more, nothing happens.
    let mut g = scen()
        .battlefield(0, &["Fear of Missing Out", "Test Bear", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .graveyard(0, &["Mox Pearl", "Island", "Test Giant", "Ancestral Recall"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let fomo = bf(&g, 0, "Fear of Missing Out");
    let bear = bf(&g, 0, "Test Bear");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    for a in [fomo, bear] {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(1));
        }
    }
    g.choose(Choice::Pass);
    until_p0_priority(&mut g, target_of(bear));
    bolt_then_resolve(&mut g, bear, |_| None);
    quiet_until(&mut g, |g| matches!(g.turn().step, Step::Main2 | Step::BeginCombat) && g.turn().step != Step::DeclareAttackers);
    assert_eq!(g.turn().step, Step::Main2, "no additional combat\n{}", log(&g));
    // Aragorn.
    let mut g = scen()
        .battlefield(0, &["Aragorn, King of Gondor", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Test Bear", "Test Wall"])
        .monarch(0)
        .at(Step::DeclareAttackers, 0)
        .start();
    let ar = bf(&g, 0, "Aragorn, King of Gondor");
    let bear = bf(&g, 1, "Test Bear");
    let wall = bf(&g, 1, "Test Wall");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(ar));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    until_p0_priority(&mut g, target_of(bear));
    bolt_then_resolve(&mut g, bear, |_| None);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::DeclareBlockers));
    assert_eq!(g.pending_decision().unwrap().player, 1);
    assert!(g.has_option(Choice::Card(ar)) || g.has_option(Choice::Card(wall)), "the Wall may block: {}", opts(&g));
    // Oath of Druids.
    let mut g = scen()
        .battlefield(0, &["Oath of Druids", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .library(0, &["Island", "Griselbrand", "Island"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Upkeep, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let lib = g.player(0).library.len();
    g.drive_until(|g| at_priority(g) && !g.stack().is_empty() && g.turn().priority == 0);
    bolt_then_resolve(&mut g, bear, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.player(0).library.len(), lib, "nothing revealed\n{}", log(&g));
    assert!(g.find_card(0, "Griselbrand", Zone::Battlefield).is_none());
}

// ---- atomic resolutions and casts (CR 608.2, 601.2) --------------------------------------------

/// From P0's priority with its spell or ability on top of the stack: both
/// pass; until that object has left the stack, nobody gets priority (its
/// decisions are answered by `answer`, else first options).
fn resolves_atomically(g: &mut Game, what: &str, answer: impl Fn(&Game) -> Option<Choice>) {
    let n = g.stack().len();
    assert!(n > 0 && at_priority(g), "{what}");
    g.pass();
    if at_priority(g) && g.turn().priority == 1 && g.stack().len() == n {
        g.pass();
    }
    let mut decided = 0;
    while g.stack().len() >= n {
        let d = g.pending_decision().unwrap_or_else(|| panic!("{what}: game over"));
        assert_ne!(d.kind, DecisionKind::Priority, "{what}: priority in the middle of a resolution\n{}", log(g));
        let c = answer(g).unwrap_or(d.options[0].choice);
        g.choose(c);
        decided += 1;
    }
    let _ = decided;
}

#[test]
fn no_player_can_act_in_the_middle_of_a_resolution_cr_608_2() {
    // ruling: 953f3cb9 c83041cc 7614aeed 6082220c ba621a28 28672f55 0f0a2aaf 78ef5708 9da53bfb e9521273 d1cf4160
    // Brainstorm, Jace's 0, Frantic Search, Faithless Looting, Dack's +1,
    // Council's Judgment (the votes), Mystic and Fiery Confluence, Three
    // Steps Ahead, Demonic Consultation (the name is chosen as it resolves,
    // after P1 had priority with it on the stack): every decision made
    // while the object resolves, none with priority between.
    let cases: Vec<(&str, &[&str], &[&str])> = vec![
        ("Brainstorm", &["Island"], &[]),
        ("Frantic Search", &["Island", "Island", "Island"], &[]),
        ("Faithless Looting", &["Mountain"], &[]),
        ("Council's Judgment", &["Plains", "Plains", "Plains"], &["Sol Ring", "Test Bear"]),
        ("Mystic Confluence", &["Island"; 5], &["Test Bear"]),
        ("Fiery Confluence", &["Mountain"; 4], &["Test Bear", "Sol Ring"]),
        ("Three Steps Ahead", &["Island"; 3], &[]),
        ("Demonic Consultation", &["Swamp"], &[]),
    ];
    for (spell, lands, theirs) in cases {
        let mut g = scen().battlefield(0, lands).hand(0, &[spell, "Island", "Forest"]).battlefield(1, theirs).at(Step::Main1, 0).start();
        let c = g.card(0, spell, Zone::Hand);
        g.choose(Choice::Cast(c));
        until_p0_priority(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMode) if spell == "Three Steps Ahead" => g.has_option(Choice::Number(2)).then_some(Choice::Number(2)).or(Some(Choice::Pass)),
            _ => None,
        });
        if spell == "Demonic Consultation" {
            g.pass();
            assert!(at_priority(&g) && g.turn().priority == 1, "P1 has priority with it on the stack, no name chosen yet");
            g.pass();
            assert_ne!(g.kind(), Some(DecisionKind::Priority));
            continue;
        }
        resolves_atomically(&mut g, spell, |_| None);
    }
    // Jace's 0 and Dack's +1.
    for (walker, slot) in [("Jace, the Mind Sculptor", 1u8), ("Dack Fayden", 0)] {
        let mut g = scen().battlefield(0, &[walker]).hand(0, &["Island", "Forest"]).at(Step::Main1, 0).start();
        let w = bf(&g, 0, walker);
        g.choose(ab(w, slot));
        until_p0_priority(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(0)));
        resolves_atomically(&mut g, walker, |_| None);
    }
}

#[test]
fn explore_connive_and_cori_steels_attach_choice_happen_within_one_resolution_cr_608_2() {
    // ruling: ee85216a b5fe5fd5 379a1727 d0803303
    // A Map's explore (reveal, then keep or bin), Raffine's connive (draw,
    // discard, counter), Cori-Steel Cutter's Monk and "you may attach",
    // Mana Crypt's flip and its damage: no priority within.
    // (The Sentinel's Map comes when it enters or attacks: cast it.)
    let mut g = scen().battlefield(0, &["Forest", "Forest", "Forest", "Island"]).hand(0, &["Sentinel of the Nameless City"]).library(0, &["Lightning Bolt", "Island"]).at(Step::Main1, 0).start();
    let s = g.card(0, "Sentinel of the Nameless City", Zone::Hand);
    g.choose(Choice::Cast(s));
    resolve(&mut g);
    let map = tokens_named(&g, "Map")[0];
    g.choose(ab(map, 0));
    until_p0_priority(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(s)));
    resolves_atomically(&mut g, "explore", |_| None);
    // Raffine: "Whenever you attack, target attacking creature connives X."
    let mut g = scen().battlefield(0, &["Raffine, Scheming Seer"]).at(Step::DeclareAttackers, 0).start();
    let raf = bf(&g, 0, "Raffine, Scheming Seer");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(raf));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    until_p0_priority(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(raf)));
    resolves_atomically(&mut g, "connive", |_| None);
    // Cori-Steel Cutter.
    let mut g = scen().battlefield(0, &["Cori-Steel Cutter", "Mountain", "Mountain"]).hand(0, &["Lightning Bolt", "Lightning Bolt"]).at(Step::Main1, 0).start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    cast_targets(&mut g, b, &[player(1)]);
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    until_p0_priority(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)));
    resolves_atomically(&mut g, "flurry", |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    // Mana Crypt at P0's upkeep.
    let mut g = scen().battlefield(0, &["Mana Crypt"]).at(Step::Upkeep, 0).start();
    g.drive_until(|g| at_priority(g) && !g.stack().is_empty());
    resolves_atomically(&mut g, "Mana Crypt", |_| None);
}

/// Cast `c` (or activate, with `act`), asserting that until it is on the
/// stack and P0 has priority again nobody else got priority.
fn casts_atomically(g: &mut Game, first: Choice, answer: impl Fn(&Game) -> Option<Choice>) {
    let n = g.stack().len();
    g.choose(first);
    while !(at_priority(g) && g.stack().len() > n) {
        let d = g.pending_decision().unwrap();
        assert_ne!(d.kind, DecisionKind::Priority, "priority while casting\n{}", log(g));
        assert_eq!(d.player, 0, "only the caster decides while casting: {:?}", d.kind);
        let c = answer(g).unwrap_or(d.options[0].choice);
        g.choose(c);
    }
    assert_eq!(g.turn().priority, 0);
}

#[test]
fn casting_and_paying_is_never_interrupted_cr_601_2() {
    // ruling: c0295cda 0a241466 b2be720c d4f53c05 4f616c51 db2bccf4
    // Bone Shards (sacrificing a creature), Natural Order (sacrificing a
    // green creature), Crabomination's emerge (sacrificing an artifact), an
    // escape from Underworld Breach (exiling cards), Emry (its reduction
    // counted from the artifacts), crewing Smuggler's Copter: every choice
    // and payment is made before anyone gets priority.
    let mut g = scen().battlefield(0, &["Swamp", "Test Bear"]).battlefield(1, &["Test Wall"]).hand(0, &["Bone Shards"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Bone Shards", Zone::Hand);
    let bear = bf(&g, 0, "Test Bear");
    casts_atomically(&mut g, Choice::Cast(c), |g| match g.kind() {
        Some(DecisionKind::ChooseCost) => Some(Choice::Number(0)),
        Some(DecisionKind::Sacrifice) => Some(Choice::Card(bear)),
        _ => None,
    });
    let mut g = scen().battlefield(0, &["Forest"; 4]).battlefield(0, &["Llanowar Elves"]).library(0, &["Test Giant", "Island"]).hand(0, &["Natural Order"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Natural Order", Zone::Hand);
    casts_atomically(&mut g, Choice::Cast(c), |_| None);
    let mut g = scen().battlefield(0, &["Swamp"; 5]).battlefield(0, &["Mind Stone"]).hand(0, &["Crabomination"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Crabomination", Zone::Hand);
    casts_atomically(&mut g, Choice::Cast(c), |g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(1)));
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Mountain"])
        .graveyard(0, &["Lightning Bolt", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Lightning Bolt", Zone::Graveyard);
    casts_atomically(&mut g, Choice::Cast(c), |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)));
    let mut g = scen().battlefield(0, &["Island", "Sol Ring", "Mox Pearl"]).hand(0, &["Emry, Lurker of the Loch"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Emry, Lurker of the Loch", Zone::Hand);
    casts_atomically(&mut g, Choice::Cast(c), |_| None);
    let mut g = scen().battlefield(0, &["Smuggler's Copter", "Test Bear"]).at(Step::Main1, 0).start();
    let copter = bf(&g, 0, "Smuggler's Copter");
    casts_atomically(&mut g, ab(copter, 0), |_| None);
}

// ---- the active player acts first after a resolution (CR 117.3b) --------------------------------

#[test]
fn a_card_put_into_your_graveyard_during_your_turn_can_be_cast_before_the_opponent_acts_cr_117_3b() {
    // ruling: 64b09987 8e380ea1 33ad6e92 5b5e7097 de6aa02a 6c69604c
    // Faithless Looting discards Lingering Souls, a Bolt (Underworld
    // Breach), Fanatic of Rhonas and a Llanowar Elves (Lurrus): right after
    // the resolution P0 has priority and may flash back, escape, eternalize
    // and cast with Lurrus. Flashback and escape don't change timing: with a
    // spell on the stack, the sorceries can't be cast.
    for pair in [["Lingering Souls", "Lightning Bolt"], ["Fanatic of Rhonas", "Llanowar Elves"]] {
        let mut g = scen()
            .battlefield(0, &["Underworld Breach", "Lurrus of the Dream-Den", "Mountain", "Mountain", "Swamp", "Swamp", "Plains", "Forest", "Forest", "Forest", "Forest"])
            .graveyard(0, &["Island", "Island", "Island"])
            .hand(0, &["Faithless Looting", pair[0], pair[1]])
            .library(0, &["Island", "Island", "Island"])
            .at(Step::Main1, 0)
            .start();
        let fl = g.card(0, "Faithless Looting", Zone::Hand);
        let cards: Vec<CardIdx> = pair.iter().map(|n| g.card(0, n, Zone::Hand)).collect();
        g.choose(Choice::Cast(fl));
        resolve_with(&mut g, |g| {
            let d = g.pending_decision().unwrap();
            cards.iter().map(|&c| Choice::Card(c)).find(|&c| d.options.iter().any(|o| o.choice == c))
        });
        assert!(cards.iter().all(|&c| g.obj(c).zone == Zone::Graveyard), "{}", log(&g));
        assert!(at_priority(&g) && g.turn().priority == 0 && g.stack().is_empty());
        for &c in &cards {
            let ok = g.has_option(Choice::Cast(c)) || g.has_option(Choice::Ability(c, AbilitySlot::Graveyard(0)));
            assert!(ok, "{} castable at once: {}", g.name(c), opts(&g));
        }
        if pair[0] == "Lingering Souls" {
            // On P1's turn (P0 with priority in P1's upkeep): the sorcery
            // can't be flashed back.
            quiet_until(&mut g, |g| g.turn().active == 1 && at_priority(g) && g.turn().priority == 0);
            assert!(!g.has_option(Choice::Cast(cards[0])), "flashback keeps sorcery timing: {}", opts(&g));
        }
    }
}

// ---- uncounterable spells: what else the spell does (CR 101.2) ---------------------------------

#[test]
fn reprieve_returns_an_uncounterable_spell_and_mana_drain_still_gives_its_mana_cr_101_2() {
    // ruling: 5c58b83a 7c1cc9dc 360847f0
    // P1's Abrupt Decay ("can't be countered") at P0's Sol Ring: Reprieve
    // returns it to P1's hand (not a counter) and P0 draws. Mana Drain at
    // it: not countered, the Sol Ring is destroyed, and P0 still gets {C}{C}
    // at its next main phase. The same under Hexing Squelcher ("spells you
    // control can't be countered") with a Lightning Bolt.
    let mut g = scen()
        .battlefield(0, &["Sol Ring", "Plains", "Plains"])
        .hand(0, &["Reprieve"])
        .battlefield(1, &["Bayou", "Bayou"])
        .hand(1, &["Abrupt Decay"])
        .at(Step::Main1, 1)
        .start();
    let ring = bf(&g, 0, "Sol Ring");
    let ad = g.card(1, "Abrupt Decay", Zone::Hand);
    g.choose(Choice::Cast(ad));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(ring));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let hand = g.player(0).hand.len();
    let rp = g.card(0, "Reprieve", Zone::Hand);
    g.choose(Choice::Cast(rp));
    let t = g.tgt(ad);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(ad).zone, Zone::Hand, "{}", log(&g));
    assert_eq!(g.obj(ring).zone, Zone::Battlefield);
    assert_eq!(g.player(0).hand.len(), hand, "Reprieve left, a card drawn");
    for (spell, extra) in [("Abrupt Decay", None), ("Lightning Bolt", Some("Hexing Squelcher"))] {
        let mut sc = scen()
            .battlefield(0, &["Sol Ring", "Island", "Island", "Test Bear"])
            .hand(0, &["Mana Drain"])
            .battlefield(1, &["Bayou", "Bayou", "Mountain"])
            .hand(1, &[spell])
            .at(Step::Main1, 1);
        if let Some(e) = extra {
            sc = sc.battlefield(1, &[e]);
        }
        let mut g = sc.start();
        let target = if spell == "Abrupt Decay" { bf(&g, 0, "Sol Ring") } else { bf(&g, 0, "Test Bear") };
        let c = g.card(1, spell, Zone::Hand);
        g.choose(Choice::Cast(c));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(g.tgt(target));
        pay(&mut g);
        g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
        let md = g.card(0, "Mana Drain", Zone::Hand);
        g.choose(Choice::Cast(md));
        let t = g.tgt(c);
        resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
        assert_eq!(g.obj(target).zone, Zone::Graveyard, "{spell} not countered\n{}", log(&g));
        quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g) && g.stack().is_empty());
        let mv = if spell == "Abrupt Decay" { 2 } else { 1 };
        assert_eq!(g.player(0).pool.total(), mv, "{spell}: Mana Drain's mana all the same\n{}", log(&g));
    }
}

// ---- spells cast during a resolution or from other zones: timing (CR 608.2g, 601.2) ------------

#[test]
fn spells_cast_during_a_resolution_ignore_timing_and_later_casts_follow_it_cr_608_2g() {
    // ruling: 92f52ee7 d4e8981b 95b53341
    // Malcolm's combat-damage trigger (four chorus counters) casts the
    // discarded sorcery Chain Lightning during combat. Chandra's +1 casts
    // an exiled sorcery while its ability resolves (the stack holds the
    // ability). Jace, Telepath Unbound's -3 lets P0 cast a sorcery from the
    // graveyard this turn, at sorcery timing only.
    let chorus = Counter::Named(common::db().subtype_id("counter:chorus").unwrap());
    let mut g = scen()
        .battlefield(0, &["Malcolm, Alluring Scoundrel"])
        .counters(0, "Malcolm, Alluring Scoundrel", chorus, 3)
        .hand(0, &["Chain Lightning"])
        .library(0, &["Island", "Island"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let m = bf(&g, 0, "Malcolm, Alluring Scoundrel");
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(m));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main2, |g| match g.kind() {
        Some(DecisionKind::Discard) | Some(DecisionKind::ChooseDiscard) if g.has_option(Choice::Card(cl)) => Some(Choice::Card(cl)),
        Some(DecisionKind::YesNo) if g.pending_decision().unwrap().player == 0 => Some(Choice::Yes),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.life(1), 20 - 2 - 3, "Malcolm's 2, Chain Lightning's 3 in combat\n{}", log(&g));
    // Chandra's +1: the exiled top card, a sorcery, cast while the
    // ability resolves.
    let mut g = scen()
        .battlefield(0, &["Chandra, Torch of Defiance", "Mountain"])
        .library(0, &["Chain Lightning", "Island"])
        .at(Step::Main1, 0)
        .start();
    let ch = bf(&g, 0, "Chandra, Torch of Defiance");
    g.choose(ab(ch, 0));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) if g.pending_decision().unwrap().player == 0 => Some(Choice::Yes),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert_eq!(g.life(1), 17, "{}", log(&g));
    // Jace, Telepath Unbound's -3: Chain Lightning from the graveyard, not
    // in combat.
    let jace = "Jace, Vryn's Prodigy // Jace, Telepath Unbound";
    let mut g = scen()
        .battlefield(0, &[jace, "Mountain", "Mountain", "Test Bear"])
        .transformed(0, jace)
        .counters(0, "Jace, Telepath Unbound", Counter::Loyalty, 5)
        .graveyard(0, &["Chain Lightning"])
        .at(Step::Main1, 0)
        .start();
    let j = bf(&g, 0, "Jace, Telepath Unbound");
    let cl = g.card(0, "Chain Lightning", Zone::Graveyard);
    let a = (0..3u8).map(|i| ab(j, i)).filter(|&a| g.has_option(a)).last().expect("-3");
    g.choose(a);
    let t = g.tgt(cl);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    let bear = bf(&g, 0, "Test Bear");
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers) || (at_priority(g) && g.turn().step == Step::BeginCombat));
    if at_priority(&g) {
        assert!(!g.has_option(Choice::Cast(cl)), "a sorcery: not in combat");
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(bear));
    if g.kind() == Some(DecisionKind::AttackTarget) {
        g.choose(player(1));
    }
    g.choose(Choice::Pass);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    assert!(!g.has_option(Choice::Cast(cl)), "not in combat: {}", opts(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
    assert!(g.has_option(Choice::Cast(cl)), "in the main phase: {}", opts(&g));
}

// ---- copies on the stack (CR 707.10) -----------------------------------------------------------

#[test]
fn storm_and_replicate_copies_dont_need_their_original_and_each_takes_its_own_target_cr_707_10() {
    // ruling: c33f0cb3 bf7e6c38 f9e25ba1
    // Brain Freeze with storm 2 (two spells before): one copy at P0, one at
    // P1, the original at P1. P1 counters the original (Counterspell): the
    // copies still resolve. Lose Focus replicated once, countered: its
    // replicate trigger still copies it.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Mishra's Bauble", "Mishra's Bauble", "Brain Freeze"])
        .library(0, &["Island"; 20])
        .library(1, &["Forest"; 20])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    for _ in 0..2 {
        let b = g.card(0, "Mishra's Bauble", Zone::Hand);
        g.choose(Choice::Cast(b));
        resolve(&mut g);
    }
    let bf_ = g.card(0, "Brain Freeze", Zone::Hand);
    g.choose(Choice::Cast(bf_));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    // The storm trigger: two copies, new targets: P0, P1.
    let k = std::cell::Cell::new(0);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0 && g.stack().len() == 1);
    g.pass();
    let (l0, l1) = (g.player(0).library.len(), g.player(1).library.len());
    // P1 lets the storm trigger resolve, then counters the original.
    quiet_until_with(&mut g, |g| g.stack().len() == 3 && at_priority(g) && g.turn().priority == 1, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::NewTarget) | Some(DecisionKind::Target) => {
            k.set(k.get() + 1);
            Some(if k.get() % 2 == 1 { player(0) } else { player(1) })
        }
        _ => None,
    });
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(bf_);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(bf_).zone, Zone::Graveyard, "countered");
    assert_eq!((l0 - g.player(0).library.len(), l1 - g.player(1).library.len()), (3, 3), "the copies: P0 and P1, three each\n{}", log(&g));
    // Lose Focus replicated once, countered by P1's Counterspell first.
    let mut g = scen()
        .battlefield(0, &["Island"; 4])
        .hand(0, &["Lose Focus"])
        .battlefield(1, &["Island", "Island", "Mountain"])
        .hand(1, &["Counterspell", "Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let b = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(0));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let lf = g.card(0, "Lose Focus", Zone::Hand);
    g.choose(Choice::Cast(lf));
    quiet_until_with(&mut g, |g| at_priority(g) && g.turn().priority == 1, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) | Some(DecisionKind::ChooseCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(b)),
        _ => None,
    });
    // The replicate trigger is on top; P1 counters Lose Focus under it.
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(lf);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(lf).zone, Zone::Graveyard);
    assert_eq!(g.obj(b).zone, Zone::Graveyard);
    assert_eq!(g.life(0), 20, "the copy countered the Bolt (P1 didn't pay)\n{}", log(&g));
}

#[test]
fn a_copy_returned_to_hand_ceases_to_exist_and_chain_lightnings_copy_isnt_cast_cr_707_10a() {
    // ruling: fb03cf68 c0f2a5ef
    // Sink into Stupor at Lutri's copy of P0's Lightning Bolt: the copy
    // goes to P0's hand and ceases to exist. P1 copies Chain Lightning
    // under Saheeli ("whenever you cast a noncreature spell"): no Servo;
    // P0 may respond to the copy.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Island", "Island", "Island"])
        .hand(0, &["Lightning Bolt", "Lutri, the Spellchaser"])
        .battlefield(1, &["Island", "Island", "Island"])
        .hand(1, &["Sink into Stupor // Soporific Springs"])
        .at(Step::Main1, 0)
        .start();
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let l = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(l));
    quiet_until_with(&mut g, |g| at_priority(g) && g.turn().priority == 1 && g.log_lines().iter().any(|x| x.contains("copies Lightning Bolt")), |g| match g.kind() {
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    let hand = g.player(0).hand.len();
    let sink = g.card(1, "Sink into Stupor // Soporific Springs", Zone::Hand);
    g.choose(Choice::Cast(sink));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| matches!(c, Choice::Target(engine::Target::Obj(r)) if g.is_copy_obj(r.idx))),
        _ => None,
    });
    assert_eq!(g.player(0).hand.len(), hand, "the copy ceased to exist in the hand\n{}", log(&g));
    assert_eq!(g.life(1), 17, "the original resolved");
    // Chain Lightning copied by P1 (Saheeli's): no Servo; P0 may respond.
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Chain Lightning"])
        .battlefield(1, &["Saheeli, Sublime Artificer", "Mountain", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let cl = g.card(0, "Chain Lightning", Zone::Hand);
    g.choose(Choice::Cast(cl));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    let mut p0_could_respond = false;
    for _ in 0..200 {
        if g.stack().is_empty() && at_priority(&g) {
            break;
        }
        let d = g.pending_decision().unwrap();
        if d.kind == DecisionKind::Priority && d.player == 0 && !g.stack().is_empty() && g.obj(cl).zone != Zone::Stack {
            p0_could_respond = true;
        }
        let c = match d.kind {
            DecisionKind::YesNo if d.player == 1 => Choice::Yes,
            DecisionKind::YesNo => Choice::Pass,
            DecisionKind::Target | DecisionKind::NewTarget if g.has_option(player(0)) => player(0),
            DecisionKind::Priority => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    assert_eq!(g.life(0), 17, "P1's copy hit P0: {}", log(&g));
    assert!(p0_could_respond, "the copy was on the stack with P0's priority");
    assert!(tokens_named(&g, "Servo").is_empty(), "the copy wasn't cast");
}

// ---- abilities on the stack, special actions (CR 602.2, 116.2) ---------------------------------

#[test]
fn class_levels_and_candelabra_use_the_stack_plot_and_the_companion_dont_cr_602_2_116_2() {
    // ruling: f1a0fb40 450c2022 159f8a4d a4cb7594 5a3cfad1 3d37a052
    // Does Machines's level-up and Candelabra of Tawnos's untap are
    // abilities on the stack: P1 gets priority with them waiting. Plotting
    // Pillage the Bog is a special action: nothing on the stack, P0 still
    // has priority. The companion put into hand for {3} (a special action)
    // is then an ordinary card: cast and countered, it goes to the graveyard.
    let mut g = scen().battlefield(0, &["Does Machines", "Island", "Island"]).at(Step::Main1, 0).start();
    let dm = bf(&g, 0, "Does Machines");
    g.choose(ab(dm, 0));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.stack().len(), 1, "the level-up waits on the stack\n{}", log(&g));
    let mut g = scen().battlefield(0, &["Candelabra of Tawnos", "Island", "Island"]).at(Step::Main1, 0).start();
    let ca = bf(&g, 0, "Candelabra of Tawnos");
    let isl = bf(&g, 0, "Island");
    g.choose(ab(ca, 0));
    quiet_until_with(&mut g, |g| at_priority(g) && g.turn().priority == 1, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(g.tgt(isl)),
        _ => None,
    });
    assert_eq!(g.stack().len(), 1, "Candelabra's ability waits\n{}", log(&g));
    let mut g = scen().battlefield(0, &["Bayou", "Bayou"]).hand(0, &["Pillage the Bog"]).at(Step::Main1, 0).start();
    let pb = g.card(0, "Pillage the Bog", Zone::Hand);
    let plot = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Cast(pb) && c != Choice::Pass && g.describe_choice(c).contains("Pillage"));
    if let Some(p) = plot {
        g.choose(p);
        pay(&mut g);
        assert!(g.stack().is_empty() && at_priority(&g) && g.turn().priority == 0, "{}", log(&g));
        assert_eq!(g.obj(pb).zone, Zone::Exile);
    }
    // The companion.
    let mut g = scen()
        .battlefield(0, &["Plains"; 6])
        .companion(0, "Lurrus of the Dream-Den")
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    g.choose(Choice::Companion);
    pay(&mut g);
    let lurrus = g.card(0, "Lurrus of the Dream-Den", Zone::Hand);
    assert!(g.stack().is_empty() && g.turn().priority == 0, "no stack, P0 still acts");
    g.choose(Choice::Cast(lurrus));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(lurrus);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(lurrus).zone, Zone::Graveyard, "an ordinary card now\n{}", log(&g));
}

// ---- splice (CR 702.47) ------------------------------------------------------------------------

#[test]
fn splice_reveals_other_cards_once_each_and_targets_are_chosen_per_instance_cr_702_47() {
    // ruling: e8d43bb3 63859cb3 c4b9b754 0fbdcacf 913357b1 15eab260
    // Goryo's Vengeance alone: no splice offered (it can't be spliced onto
    // itself). Two Goryo's: the other is spliced once, and each "target"
    // gets its own: Griselbrand and Atraxa both return. Through the Breach:
    // putting a creature onto the battlefield is optional.
    let mut g = scen().battlefield(0, &["Badlands"; 8]).hand(0, &["Goryo's Vengeance"]).graveyard(0, &["Griselbrand"]).at(Step::Main1, 0).start();
    let gv = g.card(0, "Goryo's Vengeance", Zone::Hand);
    g.choose(Choice::Cast(gv));
    g.drive_until(|g| !matches!(g.kind(), Some(DecisionKind::Priority)));
    if g.kind() == Some(DecisionKind::ChooseHandCard) {
        assert_eq!(g.pending_decision().unwrap().options.len(), 1, "nothing to splice but no: {}", opts(&g));
    }
    let mut g = scen()
        .battlefield(0, &["Badlands"; 8])
        .hand(0, &["Goryo's Vengeance", "Goryo's Vengeance"])
        .graveyard(0, &["Griselbrand", "Atraxa, Grand Unifier"])
        .at(Step::Main1, 0)
        .start();
    let gris = g.card(0, "Griselbrand", Zone::Graveyard);
    let atr = g.card(0, "Atraxa, Grand Unifier", Zone::Graveyard);
    let gvs: Vec<CardIdx> = g.player(0).hand.clone();
    g.choose(Choice::Cast(gvs[0]));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    assert!(g.has_option(Choice::Card(gvs[1])) && !g.has_option(Choice::Card(gvs[0])), "the other one only");
    g.choose(Choice::Card(gvs[1]));
    g.drive_until(|g| g.kind() != Some(DecisionKind::ChooseHandCard) || g.pending_decision().unwrap().options.len() == 1);
    if g.kind() == Some(DecisionKind::ChooseHandCard) {
        g.choose(Choice::Pass);
    }
    let picked = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => {
            picked.set(picked.get() + 1);
            let want = if picked.get() == 1 { gris } else { atr };
            g.has_option(g.tgt(want)).then(|| g.tgt(want))
        }
        Some(DecisionKind::YesNo) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.obj(gris).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(atr).zone, Zone::Battlefield, "a different target for the spliced instance\n{}", log(&g));
    // Through the Breach: may put nothing.
    let mut g = scen().battlefield(0, &["Badlands"; 5]).hand(0, &["Through the Breach", "Test Bear"]).at(Step::Main1, 0).start();
    let tb = g.card(0, "Through the Breach", Zone::Hand);
    g.choose(Choice::Cast(tb));
    let asked = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => {
            asked.set(true);
            assert!(g.has_option(Choice::Pass), "optional: {}", opts(g));
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert!(asked.get());
    assert_eq!(g.count_in(0, "Test Bear", Zone::Hand), 1, "{}", log(&g));
}

// ---- modes and targets (CR 700.2, 115.3) -------------------------------------------------------

#[test]
fn two_modes_of_collective_brutality_may_target_the_same_opponent_and_lutris_kolaghans_copy_keeps_its_modes_cr_700_2d_707_10c() {
    // ruling: 1f109819 4b30736a
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Collective Brutality", "Island", "Island"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let cb = g.card(0, "Collective Brutality", Zone::Hand);
    g.choose(Choice::Cast(cb));
    let modes = std::cell::Cell::new(0);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            modes.set(modes.get() + 1);
            let c = [Choice::Number(0), Choice::Number(2), Choice::Pass][(modes.get() - 1).min(2)];
            g.has_option(c).then_some(c).or(Some(Choice::Pass))
        }
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::DiscardCost) | Some(DecisionKind::ChooseCost) | Some(DecisionKind::Discard) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        Some(DecisionKind::ChooseHandCard) | Some(DecisionKind::ChooseDiscard) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        _ => None,
    });
    assert_eq!(g.life(1), 18, "{}", log(&g));
    assert_eq!(g.count_in(1, "Lightning Bolt", Zone::Graveyard), 1, "both modes at P1\n{}", log(&g));
    // Lutri copying Kolaghan's Command (2 damage to the Bear, destroy the
    // Sol Ring): the copy has the same modes, new targets allowed: P1.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Swamp", "Island", "Island", "Island"])
        .hand(0, &["Kolaghan's Command", "Lutri, the Spellchaser"])
        .battlefield(1, &["Sol Ring", "Mox Pearl", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let ring = bf(&g, 1, "Sol Ring");
    let pearl = bf(&g, 1, "Mox Pearl");
    let bear = bf(&g, 1, "Test Bear");
    let kc = g.card(0, "Kolaghan's Command", Zone::Hand);
    g.choose(Choice::Cast(kc));
    let modes = std::cell::Cell::new(0);
    until_p0_priority(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            modes.set(modes.get() + 1);
            Some(Choice::Number(if modes.get() == 1 { 2 } else { 3 }))
        }
        Some(DecisionKind::Target) if g.has_option(g.tgt(ring)) => Some(g.tgt(ring)),
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    let l = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(l));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) | Some(DecisionKind::NewTarget) if g.has_option(g.tgt(pearl)) => Some(g.tgt(pearl)),
        Some(DecisionKind::Target) | Some(DecisionKind::NewTarget) if g.has_option(player(1)) => Some(player(1)),
        Some(DecisionKind::Target) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert_eq!(g.obj(pearl).zone, Zone::Graveyard, "the copy's new artifact target\n{}", log(&g));
    assert_eq!(g.life(1), 18, "the copy's 2 damage at P1\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "the original's 2 damage");
}

// ---- hidden information during a cast (CR 601.2, Bolas's Citadel) --------------------------------

#[test]
fn bolass_citadel_the_new_top_card_is_unseen_until_the_cast_is_done_cr_601_2() {
    // ruling: 2012f8f3
    let mut g = scen()
        .battlefield(0, &["Bolas's Citadel"])
        .library(0, &["Lightning Bolt", "Ancestral Recall", "Island"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    let ar = g.card(0, "Ancestral Recall", Zone::Library);
    assert!(g.knows(0, bolt));
    g.choose(Choice::Cast(bolt));
    let mut seen_mid = false;
    while !(at_priority(&g) && !g.stack().is_empty()) {
        if g.knows(0, ar) {
            seen_mid = true;
        }
        let d = g.pending_decision().unwrap();
        let c = if d.kind == DecisionKind::Target { player(1) } else { d.options[0].choice };
        g.choose(c);
    }
    assert!(!seen_mid, "the next card isn't seen while casting");
    assert!(g.knows(0, ar), "seen once the cast is done\n{}", log(&g));
}

// ---- Pyroblast redirected (CR 115.7) -----------------------------------------------------------

#[test]
fn a_redirected_pyroblast_keeps_its_counter_mode_cr_700_2_115_7() {
    // ruling: fa2aaa26
    // P1's Pyroblast (counter mode) at P0's Brainstorm; P0's Invert
    // Polarity wins the flip and takes it: its new target must be a spell,
    // not a permanent.
    use engine::hidden::{key, KEY_COIN};
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Mountain", "Sol Ring"])
        .hand(0, &["Brainstorm", "Invert Polarity"])
        .battlefield(1, &["Mountain", "Island"])
        .hand(1, &["Pyroblast"])
        .at(Step::Main1, 0)
        .start();
    let bs = g.card(0, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    pay(&mut g);
    g.pass();
    let pb = g.card(1, "Pyroblast", Zone::Hand);
    g.choose(Choice::Cast(pb));
    quiet_until_with(&mut g, at_priority, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => Some(g.tgt(bs)),
        _ => None,
    });
    g.pass();
    let ip = g.card(0, "Invert Polarity", Zone::Hand);
    g.choose(Choice::Cast(ip));
    let t = g.tgt(pb);
    quiet_until_with(&mut g, at_priority, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    g.pass();
    g.force_chance(&[key(KEY_COIN, 1, 1)]);
    g.pass();
    let ring = bf(&g, 0, "Sol Ring");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::NewTarget) | Some(DecisionKind::Target) => {
            assert!(!g.has_option(g.tgt(ring)), "not a permanent: {}", opts(g));
            None
        }
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(pb).zone, Zone::Graveyard, "{}", log(&g));
}

// ---- triggers one can respond to; the legend rule before priority (CR 603.6, 704.3) -------------

#[test]
fn worldspine_wurms_shuffle_can_be_answered_and_a_second_mox_opal_meets_the_legend_rule_first_cr_603_3_704_3() {
    // ruling: ce2089b1 235f6424
    // Worldspine Wurm dies (Snuff Out can't: it's green; a Bolt can't: so
    // Entomb it): its shuffle trigger waits; Ghost Vacuum exiles it in
    // response: it isn't shuffled. A second Mox Opal (the third artifact)
    // goes to the graveyard (legend rule) before anyone has priority: one
    // Mox Opal to tap.
    let mut g = scen()
        .battlefield(0, &["Ghost Vacuum", "Swamp"])
        .hand(0, &["Entomb"])
        .library(0, &["Worldspine Wurm", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let en = g.card(0, "Entomb", Zone::Hand);
    g.choose(Choice::Cast(en));
    let ww = g.card(0, "Worldspine Wurm", Zone::Library);
    quiet_until_with(&mut g, |g| at_priority(g) && g.obj(ww).zone == Zone::Graveyard && !g.stack().is_empty() && g.turn().priority == 0, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => Some(Choice::Def(id("Worldspine Wurm"))),
        _ => None,
    });
    let gv = bf(&g, 0, "Ghost Vacuum");
    g.choose(ab(gv, 0));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(g.tgt(ww))).then(|| g.tgt(ww)));
    assert_eq!(g.obj(ww).zone, Zone::Exile, "{}", log(&g));
    // Mox Opal.
    let mut g = scen().battlefield(0, &["Mox Opal", "Sol Ring"]).hand(0, &["Mox Opal"]).at(Step::Main1, 0).start();
    let m2 = g.card(0, "Mox Opal", Zone::Hand);
    g.choose(Choice::Cast(m2));
    quiet_until_with(&mut g, |g| at_priority(g) && g.stack().is_empty(), |g| (g.kind() == Some(DecisionKind::LegendRule)).then(|| g.pending_decision().unwrap().options[0].choice));
    assert_eq!(g.count_in(0, "Mox Opal", Zone::Battlefield), 1, "{}", log(&g));
}

// ---- Once Upon a Time, Sensei's Divining Top, Library of Alexandria ---------------------------

#[test]
fn once_upon_a_time_in_the_first_upkeep_top_in_response_to_itself_library_checked_on_activation_cr_117_602_2() {
    // ruling: f8ed4a40 3400bc39 edc8892b
    // Turn 1's upkeep: Once Upon a Time is cast for free before any land is
    // played. Sensei's Divining Top: its draw ability in response to its
    // look ability: P0 draws, Top goes on top, then the look sees it among
    // the three. Library of Alexandria with seven cards; in response P0
    // plays... casts a card (six now): it still draws.
    let mut g = scen().hand(0, &["Once Upon a Time"]).library(0, &["Island"; 8]).at(Step::Upkeep, 0).turn_number(1).start();
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let ou = g.card(0, "Once Upon a Time", Zone::Hand);
    assert_eq!(g.turn().step, Step::Upkeep);
    assert!(g.has_option(Choice::Cast(ou)), "free, in the first upkeep: {}", opts(&g));
    // Top.
    let mut g = scen().battlefield(0, &["Sensei's Divining Top", "Island"]).library(0, &["Island", "Forest", "Plains"]).at(Step::Main1, 0).start();
    let top = bf(&g, 0, "Sensei's Divining Top");
    g.choose(ab(top, 0));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0 && !g.stack().is_empty());
    let hand = g.player(0).hand.len();
    assert!(g.has_option(ab(top, 1)), "{}", opts(&g));
    g.choose(ab(top, 1));
    resolve(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 1);
    assert_eq!(g.obj(top).zone, Zone::Library, "{}", log(&g));
    // Library of Alexandria.
    let mut g = scen()
        .battlefield(0, &["Library of Alexandria", "Mountain"])
        .hand(0, &["Lightning Bolt", "Island", "Island", "Island", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let loa = bf(&g, 0, "Library of Alexandria");
    let a = (0..3u8).map(|i| ab(loa, i)).find(|&a| g.has_option(a)).expect("seven cards");
    g.choose(a);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0 && !g.stack().is_empty());
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| player(1)));
    assert_eq!(g.player(0).hand.len(), 7, "six, then the draw\n{}", log(&g));
}

// ---- cascade (CR 702.85) -----------------------------------------------------------------------

#[test]
fn cascade_resolves_first_even_with_its_spell_countered_and_its_exiling_isnt_optional_cr_702_85a() {
    // ruling: acd60f05 8a83c9d0 4f3bb425
    // Bloodbraid Challenger (P0) countered by P1's Counterspell in response
    // to its cascade trigger: the cascade still resolves; it exiles until a
    // Bolt (no choice to stop), and the Bolt cast goes on the stack above
    // where Bloodbraid was.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Forest", "Forest", "Forest"])
        .hand(0, &["Bloodbraid Challenger"])
        .library(0, &["Forest", "Lightning Bolt", "Island"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let bb = g.card(0, "Bloodbraid Challenger", Zone::Hand);
    g.choose(Choice::Cast(bb));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.stack().len(), 2, "the spell and its cascade trigger above it");
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(bb);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        _ => None,
    });
    assert_eq!(g.obj(bb).zone, Zone::Graveyard, "countered");
    assert_eq!(g.life(1), 17, "the cascade resolved and the Bolt was cast\n{}", log(&g));
}
