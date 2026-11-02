//! Review probes (not part of the commit): Murktide Regent / Prismari Charm edge cases.

use engine::db::{CardDb, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const MURKTIDE: &str = "Murktide Regent";
const CHARM: &str = "Prismari Charm";

fn scen() -> Scenario {
    Scenario::new(CardDb::constructed()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}
fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}
fn p1p1(g: &Game, c: CardIdx) -> u32 {
    g.counters_on(c, Counter::P1P1)
}
fn gy(g: &Game, p: PlayerId) -> Vec<CardIdx> {
    g.player(p).graveyard.clone()
}
fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}
fn pay_with(g: &mut Game, picks: &[CardIdx], answer: impl Fn(&Game) -> Option<Choice>) -> usize {
    let mut i = 0;
    for _ in 0..200 {
        if at_priority(g) || g.is_over() {
            return i;
        }
        let c = if let Some(c) = answer(g) {
            c
        } else if g.kind() == Some(DecisionKind::ChooseGraveyardCard) && i < picks.len() {
            i += 1;
            Choice::Card(picks[i - 1])
        } else if g.kind() == Some(DecisionKind::ChooseGraveyardCard) && g.has_option(Choice::Pass) {
            Choice::Pass
        } else {
            g.pending_decision().unwrap().options[0].choice
        };
        g.choose(c);
    }
    panic!("no priority\n{}", g.log_lines().join("\n"));
}
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
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
fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

const MIXED: [&str; 5] = [
    "Shatterskull Smashing // Shatterskull, the Hammer Pass",
    "Life // Death",
    "Brazen Borrower // Petty Theft",
    "Island",
    "Island",
];

#[test]
fn probe_mdfc_split_adventure_counted_by_their_card_characteristics() {
    // MDFC with a sorcery front: a sorcery card (712.8a); split card of two sorceries: a sorcery
    // card (709.4); an adventurer: a creature card (715.4). Expect 2.
    let mut g = scen().battlefield(0, &["Island", "Island"]).graveyard(0, &MIXED).hand(0, &[MURKTIDE]).at(Step::Main1, 0).start();
    let picks = gy(&g, 0);
    let m = g.card(0, MURKTIDE, Zone::Hand);
    g.choose(Choice::Cast(m));
    let n = pay_with(&mut g, &picks, |_| None);
    assert_eq!(n, 5, "{}", opts(&g));
    resolve(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Battlefield);
    println!("counters: {}", p1p1(&g, m));
    assert_eq!(p1p1(&g, m), 2, "{}", g.log_lines().join("\n"));
}

#[test]
fn probe_mdfc_split_adventure_leaving_trigger_the_regent() {
    let mut g = scen().battlefield(0, &[MURKTIDE, "Island", "Island"]).graveyard(0, &MIXED).hand(0, &[MURKTIDE]).at(Step::Main1, 0).start();
    let first = g.card(0, MURKTIDE, Zone::Battlefield);
    let picks = gy(&g, 0);
    let m = g.card(0, MURKTIDE, Zone::Hand);
    g.choose(Choice::Cast(m));
    pay_with(&mut g, &picks, |_| None);
    resolve(&mut g);
    println!("first: {} second: {}", p1p1(&g, first), p1p1(&g, m));
    assert_eq!(p1p1(&g, first), 2, "{}", g.log_lines().join("\n"));
    assert_eq!(p1p1(&g, m), 2);
}

#[test]
fn probe_endurance_puts_graveyard_on_bottom_triggers_per_instant() {
    let mut g = scen()
        .battlefield(0, &[MURKTIDE, "Forest", "Forest", "Forest"])
        .graveyard(0, &["Brainstorm", "Ponder", "Island", "Lightning Bolt"])
        .hand(0, &["Endurance"])
        .at(Step::Main1, 0)
        .start();
    let m = g.card(0, MURKTIDE, Zone::Battlefield);
    let e = g.card(0, "Endurance", Zone::Hand);
    g.choose(Choice::Cast(e));
    pay_with(&mut g, &[], |_| None);
    let me = Choice::Target(Target::Player(0));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(me)).then_some(me));
    println!("gy {} counters {}", g.player(0).graveyard.len(), p1p1(&g, m));
    assert!(g.player(0).graveyard.is_empty(), "{}", g.log_lines().join("\n"));
    assert_eq!(p1p1(&g, m), 3, "{}", g.log_lines().join("\n"));
}

#[test]
fn probe_soul_guide_lantern_exiling_each_opponents_graveyard_triggers_their_regent() {
    // P0's Lantern exiles P1's graveyard: P1's Regent (P1's graveyard = "your graveyard") +2.
    let mut g = scen()
        .battlefield(0, &["Soul-Guide Lantern"])
        .battlefield(1, &[MURKTIDE])
        .graveyard(1, &["Brainstorm", "Ponder", "Island"])
        .at(Step::Main1, 0)
        .start();
    let m = g.card(1, MURKTIDE, Zone::Battlefield);
    let l = g.card(0, "Soul-Guide Lantern", Zone::Battlefield);
    let acts = g.options_desc();
    println!("{}", acts.join(" | "));
    // Find the activation that exiles each opponent's graveyard: try each of its abilities.
    let mut done = false;
    for slot in 0..3u8 {
        let mut h = g.clone();
        let c = Choice::Ability(l, engine::db::AbilitySlot::Activated(slot));
        if !h.has_option(c) {
            continue;
        }
        h.choose(c);
        pay_with(&mut h, &[], |_| None);
        resolve(&mut h);
        if h.player(1).graveyard.is_empty() {
            println!("slot {slot}: P1 gy exiled, P1 Regent counters {}", p1p1(&h, m));
            assert_eq!(p1p1(&h, m), 2, "{}", h.log_lines().join("\n"));
            done = true;
        }
    }
    assert!(done, "no Lantern activation exiled P1's graveyard: {}", opts(&g));
}

#[test]
fn probe_yawgmoths_will_cast_from_graveyard_triggers() {
    let mut g = scen()
        .battlefield(0, &[MURKTIDE, "Underground Sea", "Underground Sea", "Underground Sea", "Underground Sea"])
        .graveyard(0, &["Brainstorm"])
        .hand(0, &["Yawgmoth's Will"])
        .at(Step::Main1, 0)
        .start();
    let m = g.card(0, MURKTIDE, Zone::Battlefield);
    let y = g.card(0, "Yawgmoth's Will", Zone::Hand);
    g.choose(Choice::Cast(y));
    pay_with(&mut g, &[], |_| None);
    resolve(&mut g);
    let bs = g.card(0, "Brainstorm", Zone::Graveyard);
    let a = g.graveyard_action(0, bs);
    let mut v = Vec::new();
    g.legal_actions(&mut v);
    assert!(v.contains(&a), "{}", opts(&g));
    g.apply(a, &mut Vec::new()).unwrap();
    pay_with(&mut g, &[], |_| None);
    resolve(&mut g);
    println!("Regent counters {} ; Brainstorm zone {:?}", p1p1(&g, m), g.obj(bs).zone);
    assert_eq!(p1p1(&g, m), 1, "{}", g.log_lines().join("\n"));
}

#[test]
fn probe_charm_two_targets_not_the_same_player_and_planeswalkers_are_targets() {
    let mut g = scen()
        .battlefield(0, &["Volcanic Island", "Volcanic Island"])
        .hand(0, &[CHARM])
        .battlefield(1, &["Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar"])
        .transformed(1, "Tamiyo, Inquisitive Student // Tamiyo, Seasoned Scholar")
        .counters(1, "Tamiyo, Seasoned Scholar", Counter::Loyalty, 2)
        .at(Step::Main1, 0)
        .start();
    let t = g.card(1, "Tamiyo, Seasoned Scholar", Zone::Battlefield);
    let c = g.card(0, CHARM, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.choose(Choice::Number(1));
    let p1 = Choice::Target(Target::Player(1));
    let tt = g.tgt(t);
    let asked = std::cell::Cell::new(0);
    pay_with(&mut g, &[], |g| match g.kind() {
        Some(DecisionKind::Target) => {
            asked.set(asked.get() + 1);
            if asked.get() == 1 {
                assert!(!g.has_option(Choice::Pass), "zero targets allowed: {}", opts(g));
                assert!(g.has_option(tt), "a planeswalker isn't offered: {}", opts(g));
                Some(p1)
            } else {
                assert!(!g.has_option(p1), "the same player twice: {}", opts(g));
                Some(tt)
            }
        }
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.life(1), 19);
    assert_eq!(g.counters_on(t, Counter::Loyalty), 1);
}

#[test]
fn probe_metamorph_copying_a_regent_gets_no_etb_counters_but_triggers() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island"])
        .hand(0, &["Phyrexian Metamorph", "Soul-Guide Lantern"])
        .graveyard(0, &["Brainstorm"])
        .battlefield(1, &[MURKTIDE])
        .counters(1, MURKTIDE, Counter::P1P1, 3)
        .at(Step::Main1, 0)
        .start();
    let theirs = g.card(1, MURKTIDE, Zone::Battlefield);
    let pm = g.card(0, "Phyrexian Metamorph", Zone::Hand);
    g.choose(Choice::Cast(pm));
    pay_with(&mut g, &[], |_| None);
    let pick = Choice::Card(theirs);
    let tt = g.tgt(theirs);
    resolve_with(&mut g, |g| {
        if g.has_option(pick) { Some(pick) } else if g.has_option(tt) { Some(tt) } else if g.has_option(Choice::Yes) { Some(Choice::Yes) } else { None }
    });
    println!("{}", g.log_lines().iter().rev().take(8).cloned().collect::<Vec<_>>().join("\n"));
    assert_eq!(g.name(pm), MURKTIDE, "not a copy");
    println!("copy counters {} pt {:?}", p1p1(&g, pm), g.pt(pm));
    assert_eq!(p1p1(&g, pm), 0);
    let l = g.card(0, "Soul-Guide Lantern", Zone::Hand);
    g.choose(Choice::Cast(l));
    pay_with(&mut g, &[], |_| None);
    let bs = g.card(0, "Brainstorm", Zone::Graveyard);
    let tb = g.tgt(bs);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(tb)).then_some(tb));
    println!("copy counters after Lantern {}", p1p1(&g, pm));
    assert_eq!(p1p1(&g, pm), 1, "{}", g.log_lines().join("\n"));
    assert_eq!(p1p1(&g, theirs), 3, "their Regent: not their graveyard");
}
