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
