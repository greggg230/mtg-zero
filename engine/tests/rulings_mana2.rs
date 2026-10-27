//! M8d part 4 (ENGINE.md 40): the cube's mana rulings left after part 3: a
//! spell's mana value is its mana cost whatever was paid (CR 202.3, 601.2f:
//! alternative costs, additional costs, increases, reductions, delve,
//! Phyrexian mana; read by Mana Drain); X is 0 outside the stack (107.3b)
//! and when cast without paying the mana cost (107.3a-b); casting without
//! paying the mana cost and other alternative costs (118.9); flashback,
//! escape, emerge, waterbend and spree costs; devotion, domain and converge;
//! mana abilities that make no mana; spending restrictions (106.6); values
//! counted once as an ability resolves. `ruling: <hash8>` names the ruling
//! (docs/rulings.md).

mod common;
mod rulings_util;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
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

fn first_non_pass(g: &Game) -> Option<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Pass)
}

/// Answer `Target` decisions with `t` when it is an option (else first
/// options) and resolve the stack.
fn resolve_at(g: &mut Game, t: CardIdx) {
    let c = g.tgt(t);
    resolve_with(g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(c)).then_some(c));
}

/// P`p` casts `name` from hand, answering targets with `t`, and everything
/// resolves.
fn cast_at(g: &mut Game, p: u8, name: &str, t: CardIdx) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    resolve_at(g, t);
}

/// Declare `attackers` (at the defending player) and pass.
fn attack(g: &mut Game, attackers: &[CardIdx]) {
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    let defender = 1 - g.turn().active;
    for &a in attackers {
        g.choose(Choice::Card(a));
        if g.kind() == Some(DecisionKind::AttackTarget) {
            g.choose(player(defender));
        }
    }
    g.choose(Choice::Pass);
}

/// Can `p` pay `s` (a mana cost string) now?
fn can_pay(g: &Game, p: u8, s: &str) -> bool {
    g.can_pay(p, &engine::mana::ManaCost::parse(s).unwrap())
}

// ---- a spell's mana value is its mana cost (CR 202.3, 601.2f) -----------------------------------

/// P0's lands for the spells below: two of each basic, Mountains and Swamps
/// more.
const LANDS: [&str; 14] = [
    "Plains", "Plains", "Island", "Island", "Island", "Swamp", "Swamp", "Swamp", "Mountain", "Mountain", "Mountain", "Forest", "Forest", "Forest",
];

/// P0 casts `spell` (in its hand or graveyard), answering decisions with
/// `answer` (else: targets the first non-pass option, others the first);
/// P1 Mana Drains it. Returns the {C} P1 gets at its next main phase (the
/// spell's mana value), and P0's tapped permanents and life lost as the
/// spell was cast (what was paid).
fn drained(mut g: Game, spell: CardIdx, answer: impl Fn(&Game) -> Option<Choice>) -> (u8, usize, i32) {
    g.choose(Choice::Cast(spell));
    for _ in 0..60 {
        if at_priority(&g) && g.turn().priority == 0 && g.obj(spell).zone == Zone::Stack {
            break;
        }
        if at_priority(&g) {
            g.pass();
            continue;
        }
        let c = answer(&g).unwrap_or_else(|| match g.kind() {
            Some(DecisionKind::Target) => first_non_pass(&g).unwrap(),
            _ => g.pending_decision().unwrap().options[0].choice,
        });
        g.choose(c);
    }
    assert_eq!(g.obj(spell).zone, Zone::Stack, "{}\n{}", g.name(spell), log(&g));
    let paid = (g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 20 - g.life(0));
    g.pass();
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    let md = g.card(1, "Mana Drain", Zone::Hand);
    g.choose(Choice::Cast(md));
    let t = g.tgt(spell);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::Target) => first_non_pass(g),
        _ => None,
    });
    assert_ne!(g.obj(spell).zone, Zone::Stack, "countered\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    resolve(&mut g);
    (g.player(1).pool.amt[5], paid.0, paid.1)
}

/// P0 with `LANDS` and `extra` on the battlefield, `hand` and `gy`; P1 with
/// two Islands, Mana Drain and a Test Bear; P0's first main phase.
fn mv_scen(extra: &[&str], hand: &[&str], gy: &[&str]) -> Game {
    Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &LANDS)
        .battlefield(0, extra)
        .hand(0, hand)
        .graveyard(0, gy)
        .battlefield(1, &["Island", "Island", "Test Bear"])
        .hand(1, &["Mana Drain"])
        .at(Step::Main1, 0)
        .start()
}

/// Choose cost `alt` at a `ChooseAltCost` decision and option `opt` at a
/// `ChooseCost` (kicker, escalate) one.
fn costs(alt: u8, opt: u8) -> impl Fn(&Game) -> Option<Choice> {
    move |g: &Game| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(alt)),
        Some(DecisionKind::ChooseCost) => Some(Choice::Number(opt)),
        Some(DecisionKind::ChooseMode) => first_non_pass(g),
        Some(DecisionKind::PayPhyrexian) => Some(Choice::Number(g.pending_decision().unwrap().options.iter().filter_map(|o| match o.choice {
            Choice::Number(n) => Some(n),
            _ => None,
        }).max().unwrap())),
        _ => None,
    }
}

#[test]
fn a_spells_mana_value_is_its_mana_cost_whatever_cost_was_paid_cr_202_3_601_2f() {
    // ruling: ee340297 b7498104 c7024d04 ceea39f5 0cef3ca5 8082fca1 fcbbe43e b02c5786 373235a1
    // ruling: 4029cc96 cf12f8bf 5c4578c4 ca0c3283 d5edbfb1 a5f46a35 5fb37854 c220d94b
    // Mana Drain counters each and gives P1 {C} equal to its mana value:
    // evoked Solitude (5), Subtlety (4), Grief (4), Fury (5), Endurance
    // (3); overloaded Winds of Abandon and Damn (2); Faithless Looting (1)
    // and Echo of Eons (6) flashed back, also with Bilbo's reduction;
    // Burst Lightning kicked (1); Consult the Star Charts kicked (2);
    // Baleful Mastery for {1}{B} (4); Collective Brutality escalated (2);
    // Three Steps Ahead with a mode (1); Uro (3), Phlage (3), Bloodbraid
    // Challenger (5) and From the Catacombs (5) escaped; Treasure Cruise
    // delving seven (8); Dismember and Gitaxian Probe paid with life (3,
    // 1); Crabomination emerged (6); Leyline Binding for {W} (6); Emry with
    // affinity for two artifacts (3).
    // (The spell, its mana value, P0's permanents tapped to cast it: the
    // cost actually paid; 99: not checked.)
    let cases: Vec<(&str, u8, usize, Game, Box<dyn Fn(&Game) -> Option<Choice>>)> = vec![
        ("Solitude", 5, 0, mv_scen(&[], &["Solitude", "Test Cat"], &[]), Box::new(costs(1, 0))),
        ("Subtlety", 4, 0, mv_scen(&[], &["Subtlety", "Test Flyer"], &[]), Box::new(costs(1, 0))),
        ("Grief", 4, 0, mv_scen(&[], &["Grief", "Test Deathtouch"], &[]), Box::new(costs(1, 0))),
        ("Fury", 5, 0, mv_scen(&[], &["Fury", "Test Hasty"], &[]), Box::new(costs(1, 0))),
        ("Endurance", 3, 0, mv_scen(&[], &["Endurance", "Test Bear"], &[]), Box::new(costs(1, 0))),
        ("Winds of Abandon", 2, 6, mv_scen(&[], &["Winds of Abandon"], &[]), Box::new(costs(1, 0))),
        ("Damn", 2, 4, mv_scen(&[], &["Damn"], &[]), Box::new(costs(1, 0))),
        ("Faithless Looting", 1, 3, mv_scen(&[], &[], &["Faithless Looting"]), Box::new(costs(0, 0))),
        ("Echo of Eons", 6, 2, mv_scen(&["Bilbo, Thief in the Night"], &[], &["Echo of Eons"]), Box::new(costs(0, 0))),
        ("Burst Lightning", 1, 5, mv_scen(&[], &["Burst Lightning"], &[]), Box::new(costs(0, 1))),
        ("Consult the Star Charts", 2, 4, mv_scen(&[], &["Consult the Star Charts"], &[]), Box::new(costs(0, 1))),
        ("Baleful Mastery", 4, 2, mv_scen(&[], &["Baleful Mastery"], &[]), Box::new(costs(1, 0))),
        ("Three Steps Ahead", 1, 3, mv_scen(&[], &["Three Steps Ahead"], &[]), Box::new(|g: &Game| match g.kind() {
            Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(2)) => Some(Choice::Number(2)),
            Some(DecisionKind::ChooseMode) => Some(Choice::Pass),
            _ => None,
        })),
        ("Uro, Titan of Nature's Wrath", 3, 4, mv_scen(&[], &[], &["Uro, Titan of Nature's Wrath", "Island", "Island", "Island", "Island", "Island"]), Box::new(costs(0, 0))),
        ("Phlage, Titan of Fire's Fury", 3, 4, mv_scen(&[], &[], &["Phlage, Titan of Fire's Fury", "Island", "Island", "Island", "Island", "Island"]), Box::new(costs(0, 0))),
        ("Bloodbraid Challenger", 5, 5, mv_scen(&[], &[], &["Bloodbraid Challenger", "Island", "Island", "Island"]), Box::new(costs(0, 0))),
        ("From the Catacombs", 5, 5, mv_scen(&[], &[], &["From the Catacombs", "Test Bear", "Island", "Island", "Island", "Island", "Island"]), Box::new(costs(0, 0))),
        ("Treasure Cruise", 8, 1, mv_scen(&[], &["Treasure Cruise"], &["Island"; 7]), Box::new(|g: &Game| (g.kind() == Some(DecisionKind::ChooseGraveyardCard)).then(|| first_non_pass(g)).flatten())),
        ("Dismember", 3, 1, mv_scen(&[], &["Dismember"], &[]), Box::new(costs(0, 0))),
        ("Gitaxian Probe", 1, 0, mv_scen(&[], &["Gitaxian Probe"], &[]), Box::new(costs(0, 0))),
        ("Crabomination", 6, 99, mv_scen(&["Sol Ring"], &["Crabomination"], &[]), Box::new(costs(1, 0))),
        ("Leyline Binding", 6, 1, mv_scen(&[], &["Leyline Binding"], &[]), Box::new(costs(0, 0))),
        ("Emry, Lurker of the Loch", 3, 1, mv_scen(&["Sol Ring", "Mind Stone"], &["Emry, Lurker of the Loch"], &[]), Box::new(costs(0, 0))),
    ];
    for (name, mv, tapped, g, answer) in cases {
        let c = (0..g.objs().len())
            .map(|i| CardIdx(i as u16))
            .find(|&c| g.obj(c).owner == 0 && g.name(c) == name && matches!(g.obj(c).zone, Zone::Hand | Zone::Graveyard))
            .unwrap_or_else(|| panic!("no {name}"));
        let (got, t, life) = drained(g, c, answer);
        assert_eq!(got, mv, "{name}: its mana value");
        if tapped != 99 {
            assert_eq!(t, tapped, "{name}: the cost paid");
        }
        let life_paid = match name {
            "Dismember" => 4,
            "Gitaxian Probe" => 2,
            _ => 0,
        };
        assert_eq!(life, life_paid, "{name}: life paid");
    }
    // Collective Brutality escalated (two modes: a card discarded).
    let mut g = mv_scen(&[], &["Collective Brutality", "Island"], &[]);
    let cb = g.card(0, "Collective Brutality", Zone::Hand);
    let modes = std::cell::Cell::new(0);
    let (n, _, _) = drained(g.clone(), cb, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            modes.set(modes.get() + 1);
            Some(match modes.get() {
                1 => Choice::Number(1),
                2 => Choice::Number(2),
                _ => Choice::Pass,
            })
        }
        Some(DecisionKind::Target) => first_non_pass(g),
        _ => None,
    });
    assert_eq!(n, 2, "Collective Brutality escalated");
    let _ = &mut g;
}

// ---- X is 0 outside the stack (CR 107.3b, 202.3e) --------------------------------------------------

/// Cast `c` with X = `x` and pay (first sources); at priority with it on
/// the stack.
fn cast_x(g: &mut Game, c: CardIdx, x: u8) {
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(x));
    pay(g);
}

#[test]
fn x_is_zero_on_the_battlefield_and_in_a_graveyard_whatever_x_was_paid_cr_107_3b() {
    // ruling: aafa61cb 8044d61d 0e713983 ee3d4607
    // P1's Walking Ballista cast with X = 2 (it entered with two counters)
    // has mana value 0 on the battlefield: Fatal Push destroys it without
    // revolt, Abrupt Decay and Skyclave Apparition can take it (the
    // Apparition's Illusion, when it leaves, is 0/0), Vicious Rivalry with
    // X = 0 destroys it. Dead, in P1's graveyard, it has mana value 0:
    // Auriok Salvagers returns it ("mana value 1 or less"), Unearth too.
    let ballista_cast = |p0: &[&str], hand0: &[&str]| {
        let mut g = scen()
            .battlefield(0, p0)
            .hand(0, hand0)
            .battlefield(1, &["Island", "Island", "Island", "Island"])
            .hand(1, &["Walking Ballista"])
            .at(Step::Main1, 1)
            .start();
        let wb = g.card(1, "Walking Ballista", Zone::Hand);
        cast_x(&mut g, wb, 2);
        resolve(&mut g);
        assert_eq!(g.counters_on(wb, Counter::P1P1), 2);
        quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && g.stack().is_empty() && at_priority(g));
        (g, wb)
    };
    for spell in ["Fatal Push", "Abrupt Decay"] {
        let (mut g, wb) = ballista_cast(&["Swamp", "Swamp", "Forest"], &[spell]);
        assert!(!g.player(0).left_battlefield);
        cast_at(&mut g, 0, spell, wb);
        assert_eq!(g.obj(wb).zone, Zone::Graveyard, "{spell}: mana value 0\n{}", log(&g));
    }
    let (mut g, wb) = ballista_cast(&["Plains", "Plains", "Plains", "Plains", "Swamp", "Swamp"], &["Skyclave Apparition", "Vindicate"]);
    cast_at(&mut g, 0, "Skyclave Apparition", wb);
    assert_eq!(g.obj(wb).zone, Zone::Exile, "Skyclave: mana value 4 or less\n{}", log(&g));
    let sa = bf(&g, 0, "Skyclave Apparition");
    cast_at(&mut g, 0, "Vindicate", sa);
    assert!(tokens_named(&g, "Illusion").is_empty(), "a 0/0 Illusion: it dies at once\n{}", log(&g));
    assert!(log(&g).contains("Illusion"), "the token was made\n{}", log(&g));
    // Vicious Rivalry, X = 0.
    let (mut g, wb) = ballista_cast(&["Swamp", "Swamp", "Forest", "Forest"], &["Vicious Rivalry"]);
    let vr = g.card(0, "Vicious Rivalry", Zone::Hand);
    cast_x(&mut g, vr, 0);
    resolve(&mut g);
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "Vicious Rivalry X = 0\n{}", log(&g));
    assert_eq!(g.life(0), 20);
    // In a graveyard: Auriok Salvagers and Unearth (P1's own).
    let mut g = scen()
        .battlefield(1, &["Island", "Island", "Island", "Island", "Plains", "Plains", "Swamp", "Auriok Salvagers"])
        .hand(1, &["Walking Ballista", "Lightning Bolt", "Unearth"])
        .battlefield(1, &["Mountain"])
        .at(Step::Main1, 1)
        .start();
    let wb = g.card(1, "Walking Ballista", Zone::Hand);
    cast_x(&mut g, wb, 1);
    resolve(&mut g);
    g.choose(Choice::Cast(g.card(1, "Lightning Bolt", Zone::Hand)));
    resolve_at(&mut g, wb);
    assert_eq!(g.obj(wb).zone, Zone::Graveyard, "{}", log(&g));
    let sal = bf(&g, 1, "Auriok Salvagers");
    let t = g.tgt(wb);
    g.choose(ab(sal, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(t), "mana value 0 in the graveyard: {}", opts(&g));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(wb).zone, Zone::Hand);
}

#[test]
fn x_is_zero_in_a_library_and_a_revealed_cards_mana_value_is_its_mana_cost_cr_107_3b_202_3() {
    // ruling: 53a3d100 edf475b1 a6dd6e5a ddc42357 63a507fc e63641fa a56b34fa d9bc4e52
    // Trinket Mage ("mana value 1 or less") and Tezzeret, Cruel Captain's
    // -3 find Walking Ballista; Spellseeker ("2 or less") finds Mind Twist
    // ({X}{B}) but not Life // Death (a split card: {G} + {1}{B} = 3).
    // Dark Confidant revealing Ballista: no life lost; Caustic Bronco
    // revealing Ballista or an Island: none; Life // Death: 3.
    let names = |g: &Game| -> Vec<String> {
        let mut v: Vec<String> = g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .filter_map(|o| match o.choice {
                Choice::Def(d) => Some(db().def(d).name.to_string()),
                _ => None,
            })
            .collect();
        v.sort();
        v
    };
    for (finder, wanted) in [("Trinket Mage", vec!["Sol Ring", "Walking Ballista"]), ("Spellseeker", vec!["Mind Twist"])] {
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &["Walking Ballista", "Mind Twist", "Life // Death", "Sol Ring", "Mind Stone", "Island"])
            .library(1, &["Forest"; 5])
            .battlefield(0, &["Island", "Island", "Island"])
            .hand(0, &[finder])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, finder, Zone::Hand);
        g.choose(Choice::Cast(c));
        let found = std::cell::RefCell::new(Vec::new());
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            Some(DecisionKind::SearchLibrary) => {
                *found.borrow_mut() = names(g);
                Some(Choice::Pass)
            }
            _ => None,
        });
        let wanted: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
        assert_eq!(*found.borrow(), wanted, "{finder}\n{}", log(&g));
    }
    // Tezzeret's -3.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Walking Ballista", "Mind Stone", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Tezzeret, Cruel Captain"])
        .at(Step::Main1, 0)
        .start();
    let tz = bf(&g, 0, "Tezzeret, Cruel Captain");
    g.choose(ab(tz, 1));
    let found = std::cell::RefCell::new(Vec::new());
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => {
            *found.borrow_mut() = names(g);
            Some(Choice::Pass)
        }
        Some(DecisionKind::Target) => first_non_pass(g),
        _ => None,
    });
    assert_eq!(*found.borrow(), vec!["Walking Ballista".to_string()], "Tezzeret\n{}", log(&g));
    // Dark Confidant.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Walking Ballista", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Dark Confidant"])
        .at(Step::Untap, 0)
        .start();
    quiet_until(&mut g, |g| g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.count_in(0, "Walking Ballista", Zone::Hand), 1, "{}", log(&g));
    assert_eq!(g.life(0), 20, "Dark Confidant: mana value 0");
    // Caustic Bronco.
    for (top, loss) in [("Walking Ballista", 0), ("Island", 0), ("Life // Death", 3)] {
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &[top, "Island"])
            .library(1, &["Forest"; 5])
            .battlefield(0, &["Caustic Bronco"])
            .at(Step::DeclareAttackers, 0)
            .start();
        let b = bf(&g, 0, "Caustic Bronco");
        attack(&mut g, &[b]);
        quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
        assert_eq!(g.life(0), 20 - loss, "Bronco reveals {top}\n{}", log(&g));
    }
}

#[test]
fn lurrus_uses_the_x_chosen_for_the_spells_mana_value_cr_107_3_202_3e() {
    // ruling: 90a8974d
    // Walking Ballista in P0's graveyard: with Lurrus, cast with X = 1
    // (mana value 2), not X = 2 (mana value 4), even with mana for it.
    let mut g = scen()
        .battlefield(0, &["Lurrus of the Dream-Den", "Plains", "Plains", "Plains", "Plains"])
        .graveyard(0, &["Walking Ballista"])
        .at(Step::Main1, 0)
        .start();
    let wb = g.card(0, "Walking Ballista", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(wb)), "{}", opts(&g));
    g.choose(Choice::Cast(wb));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert!(g.has_option(Choice::Number(1)), "X = 1: {}", opts(&g));
    assert!(!g.has_option(Choice::Number(2)), "X = 2 makes mana value 4: {}", opts(&g));
    g.choose(Choice::Number(1));
    pay(&mut g);
    resolve(&mut g);
    assert_eq!(g.obj(wb).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.counters_on(wb, Counter::P1P1), 1);
}

/// At an X announcement: the only choice is 0.
fn x_zero_only(g: &Game) -> Choice {
    let d = g.pending_decision().unwrap();
    assert_eq!(d.options.iter().map(|o| o.choice).collect::<Vec<_>>(), vec![Choice::Number(0)], "X is 0\n{}", log(g));
    Choice::Number(0)
}

#[test]
fn x_is_zero_when_cast_without_paying_its_mana_cost_or_put_onto_the_battlefield_cr_107_3b() {
    // ruling: 550a74ab 6ca5d31d 0695134e 01ac337a
    // Walking Ballista with X = 0 (no counters: a 0/0 put into the
    // graveyard): cast free by Malcolm's fourth chorus counter, put onto
    // the battlefield by Flash, recast for {2} after Aang airbends it,
    // cascaded into by Bloodbraid Challenger.
    let in_gy = |g: &Game| g.count_in(0, "Walking Ballista", Zone::Graveyard) == 1;
    // Malcolm.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Walking Ballista", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Malcolm, Alluring Scoundrel"])
        .counters(0, "Malcolm, Alluring Scoundrel", Counter::Named(db().subtype_id("counter:chorus").unwrap()), 3)
        .at(Step::DeclareAttackers, 0)
        .start();
    let m = bf(&g, 0, "Malcolm, Alluring Scoundrel");
    attack(&mut g, &[m]);
    let cast = std::cell::Cell::new(false);
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::Discard) | Some(DecisionKind::ChooseHandCard) => first_non_pass(g),
        Some(DecisionKind::YesNo) => {
            cast.set(true);
            Some(Choice::Yes)
        }
        Some(DecisionKind::ChooseMode) => Some(x_zero_only(g)),
        _ => None,
    });
    assert!(cast.get() && in_gy(&g), "Malcolm\n{}", log(&g));
    assert!(log(&g).contains("Walking Ballista Stack -> Battlefield"), "it resolved as a 0/0\n{}", log(&g));
    // Flash.
    let mut g = scen().battlefield(0, &["Island", "Island"]).hand(0, &["Flash", "Walking Ballista"]).at(Step::Main1, 0).start();
    let fl = g.card(0, "Flash", Zone::Hand);
    g.choose(Choice::Cast(fl));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseHandCard) => first_non_pass(g),
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::ChooseMode) => Some(x_zero_only(g)),
        _ => None,
    });
    assert!(in_gy(&g), "Flash\n{}", log(&g));
    // Aang airbends the Ballista spell; recast for {2}.
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Island", "Island", "Island", "Island", "Plains", "Plains"])
        .hand(0, &["Walking Ballista", "Aang, Swift Savior // Aang and La, Ocean's Fury"])
        .at(Step::Main1, 0)
        .start();
    let wb = g.card(0, "Walking Ballista", Zone::Hand);
    cast_x(&mut g, wb, 1);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    let aang = g.card(0, "Aang, Swift Savior // Aang and La, Ocean's Fury", Zone::Hand);
    g.choose(Choice::Cast(aang));
    let t = g.tgt(wb);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(wb).zone, Zone::Exile, "airbent\n{}", log(&g));
    assert!(g.has_option(Choice::Cast(wb)), "{}", opts(&g));
    g.choose(Choice::Cast(wb));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(x_zero_only(g)),
        _ => None,
    });
    assert!(in_gy(&g), "Aang\n{}", log(&g));
    // Bloodbraid Challenger.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Walking Ballista", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Forest", "Forest"])
        .hand(0, &["Bloodbraid Challenger"])
        .at(Step::Main1, 0)
        .start();
    let bc = g.card(0, "Bloodbraid Challenger", Zone::Hand);
    g.choose(Choice::Cast(bc));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::ChooseMode) => Some(x_zero_only(g)),
        _ => None,
    });
    assert!(in_gy(&g), "cascade\n{}", log(&g));
}

// ---- without paying the mana cost; other alternative costs (CR 118.9) ----------------------------

/// The decisions asked while P0 casts `c` (answered by `answer`, else the
/// first option; targets: the first non-pass) up to P0's priority.
fn cast_recording(g: &mut Game, c: Choice, answer: impl Fn(&Game) -> Option<Choice>) -> Vec<DecisionKind> {
    let mut asked = Vec::new();
    g.choose(c);
    for _ in 0..60 {
        if at_priority(g) {
            break;
        }
        let k = g.kind().unwrap();
        asked.push(k);
        let ch = answer(g).unwrap_or_else(|| match k {
            DecisionKind::Target => first_non_pass(g).unwrap(),
            _ => g.pending_decision().unwrap().options[0].choice,
        });
        g.choose(ch);
    }
    asked
}

/// P0 activates Golos ({2}{W}{U}{B}{R}{G}): `top` exiled, playable free
/// this turn. P0 has `extra` permanents, `hand` in hand; P1 a Test Bear.
fn golos(top: [&str; 3], extra: &[&str], hand: &[&str]) -> Game {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &top)
        .library(1, &["Forest"; 10])
        .battlefield(0, &["Golos, Tireless Pilgrim", "Plains", "Island", "Swamp", "Mountain", "Forest", "Forest", "Forest"])
        .battlefield(0, extra)
        .hand(0, hand)
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let go = bf(&g, 0, "Golos, Tireless Pilgrim");
    g.choose(ab(go, 0));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| !matches!(c, Choice::Card(x) | Choice::Ability(x, _) if g.name(x) != "Plains" && g.name(x) != "Island" && g.name(x) != "Swamp" && g.name(x) != "Mountain" && g.name(x) != "Forest")),
        _ => None,
    });
    assert_eq!(g.exile_zone().len(), 3, "{}", log(&g));
    g
}

#[test]
fn a_free_cast_still_announces_x_paid_with_life_modes_and_escalate_costs_cr_118_9_107_3b() {
    // ruling: 38801307 77eebb68 e0fdde77
    // Golos's free casts: Toxic Deluge (X life, not in its mana cost: X is
    // chosen and paid); Three Steps Ahead (a mode at least, and its {2}
    // paid); Collective Brutality with two modes (a card discarded).
    let mut g = golos(["Toxic Deluge", "Island", "Island"], &[], &[]);
    let td = g.card(0, "Toxic Deluge", Zone::Exile);
    let asked = cast_recording(&mut g, Choice::Cast(td), |g| (g.kind() == Some(DecisionKind::ChooseMode)).then_some(Choice::Number(3)));
    assert!(asked.contains(&DecisionKind::ChooseMode), "X is chosen: {asked:?}");
    resolve(&mut g);
    assert_eq!(g.life(0), 17, "3 life paid\n{}", log(&g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
    // Three Steps Ahead: draw two, discard one (+{2}).
    let mut g = golos(["Three Steps Ahead", "Island", "Island"], &["Island", "Island"], &[]);
    let tsa = g.card(0, "Three Steps Ahead", Zone::Exile);
    g.choose(Choice::Cast(tsa));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert!(!g.has_option(Choice::Pass), "at least one mode: {}", opts(&g));
    g.choose(Choice::Number(2));
    let untapped = |g: &Game| g.permanents_of(0).filter(|&c| g.name(c) == "Island" && !g.obj(c).tapped).count();
    let before = untapped(&g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(before - untapped(&g), 2, "the mode's {{2}} paid\n{}", log(&g));
    // Collective Brutality: two modes, a card discarded.
    let mut g = golos(["Collective Brutality", "Island", "Island"], &[], &["Island"]);
    let cb = g.card(0, "Collective Brutality", Zone::Exile);
    let n = std::cell::Cell::new(0);
    cast_recording(&mut g, Choice::Cast(cb), |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            n.set(n.get() + 1);
            Some(match n.get() {
                1 => Choice::Number(1),
                2 => Choice::Number(2),
                _ => Choice::Pass,
            })
        }
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.count_in(0, "Island", Zone::Graveyard), 1, "escalate: the Island discarded\n{}", log(&g));
    assert_eq!(g.life(1), 18);
}

#[test]
fn a_free_cast_allows_no_alternative_cost_but_needs_mandatory_additional_costs_cr_118_9a_601_2b() {
    // ruling: 68a438a9 d2f219ee 5d19b09e
    // Golos's free Winds of Abandon: no overload offered (one target).
    // Golos's free Tinker: an artifact sacrificed; without one, not
    // castable. Bloodbraid Challenger cascading into Bone Shards: a card
    // discarded for it. An Adventure isn't an alternative cost: Golos's
    // Brazen Borrower casts Petty Theft free.
    let mut g = golos(["Winds of Abandon", "Island", "Island"], &[], &[]);
    let w = g.card(0, "Winds of Abandon", Zone::Exile);
    let asked = cast_recording(&mut g, Choice::Cast(w), |_| None);
    assert!(!asked.contains(&DecisionKind::ChooseAltCost), "no overload: {asked:?}");
    assert!(asked.contains(&DecisionKind::Target), "its target");
    // Tinker: an artifact sacrificed (Golos is one too).
    let mut g = golos(["Tinker", "Sol Ring", "Island"], &["Mind Stone"], &[]);
    let t = g.card(0, "Tinker", Zone::Exile);
    let ms = bf(&g, 0, "Mind Stone");
    let asked = cast_recording(&mut g, Choice::Cast(t), |g| g.has_option(Choice::Card(ms)).then_some(Choice::Card(ms)));
    assert!(asked.contains(&DecisionKind::Sacrifice), "{asked:?}\n{}", log(&g));
    assert_eq!(g.obj(ms).zone, Zone::Graveyard, "sacrificed\n{}", log(&g));
    // Bloodbraid into Bone Shards.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Bone Shards", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Forest", "Forest"])
        .hand(0, &["Bloodbraid Challenger", "Island"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bc = g.card(0, "Bloodbraid Challenger", Zone::Hand);
    let isl = g.card(0, "Island", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    g.choose(Choice::Cast(bc));
    let tb = g.tgt(bear);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        _ if g.has_option(Choice::Card(isl)) => Some(Choice::Card(isl)),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Bone Shards", Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.obj(isl).zone, Zone::Graveyard, "its additional cost paid\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    // Golos and Petty Theft.
    let mut g = golos(["Brazen Borrower // Petty Theft", "Island", "Island"], &["Sol Ring"], &[]);
    let bb = g.card(0, "Brazen Borrower // Petty Theft", Zone::Exile);
    let ring_p1 = bf(&g, 1, "Test Bear");
    let ch = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&o| matches!(o, Choice::CastFace(x) | Choice::Faces(x, _) if x == bb));
    let ch = ch.unwrap_or_else(|| panic!("Petty Theft offered: {}", opts(&g)));
    let lands = g.permanents_of(0).filter(|&c| !g.obj(c).tapped).count();
    g.choose(ch);
    if g.kind() == Some(DecisionKind::ChooseFace) {
        g.choose(Choice::Number(1));
    }
    resolve_at(&mut g, ring_p1);
    assert_eq!(g.obj(ring_p1).zone, Zone::Hand, "Petty Theft resolved\n{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&c| !g.obj(c).tapped).count(), lands, "free");
}

#[test]
fn cards_cast_from_exile_with_a_permission_may_use_alternative_costs_and_pay_additional_ones_cr_118_9_601_2b() {
    // ruling: eb35fd18 9950a418
    // Laelia's exiled Force of Will: cast for its alternative cost (1 life,
    // a blue card) to counter P0's own Bolt. Laelia's exiled Tinker: an
    // artifact must be sacrificed. Chandra's +1 exiles Crabomination: cast
    // for its emerge cost (Sol Ring sacrificed).
    let laelia = |top: &str, extra: &[&str], hand: &[&str]| {
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &[top, "Island", "Island"])
            .library(1, &["Forest"; 5])
            .battlefield(0, &["Laelia, the Blade Reforged", "Mountain", "Island", "Island", "Island"])
            .battlefield(0, extra)
            .hand(0, hand)
            .at(Step::DeclareAttackers, 0)
            .start();
        let l = bf(&g, 0, "Laelia, the Blade Reforged");
        attack(&mut g, &[l]);
        quiet_until(&mut g, |g| g.turn().step == Step::Main2 && at_priority(g));
        let c = g.card(0, top, Zone::Exile);
        (g, c)
    };
    let (mut g, fow) = laelia("Force of Will", &[], &["Lightning Bolt", "Brainstorm"]);
    let b = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(b));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
    assert!(g.has_option(Choice::Cast(fow)), "{}", opts(&g));
    let tb = g.tgt(b);
    let asked = cast_recording(&mut g, Choice::Cast(fow), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(tb),
        _ => None,
    });
    assert!(asked.contains(&DecisionKind::ChooseAltCost), "{asked:?}");
    resolve(&mut g);
    assert_eq!(g.life(0), 19, "1 life\n{}", log(&g));
    assert_eq!(g.count_in(0, "Brainstorm", Zone::Exile), 1, "a blue card exiled");
    assert_eq!(g.life(1), 17, "Laelia's 3; the Bolt countered\n{}", log(&g));
    // Tinker.
    let (g, t) = laelia("Tinker", &[], &[]);
    assert!(!g.has_option(Choice::Cast(t)), "no artifact: {}", opts(&g));
    let (mut g, t) = laelia("Tinker", &["Mind Stone"], &[]);
    cast_recording(&mut g, Choice::Cast(t), |_| None);
    assert_eq!(g.count_in(0, "Mind Stone", Zone::Graveyard), 1, "{}", log(&g));
    // Chandra and Crabomination.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Crabomination", "Island"])
        .library(1, &["Island", "Forest"])
        .battlefield(0, &["Chandra, Torch of Defiance", "Sol Ring", "Swamp", "Swamp", "Swamp", "Swamp"])
        .hand(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let ch = bf(&g, 0, "Chandra, Torch of Defiance");
    let ring = bf(&g, 0, "Sol Ring");
    g.choose(ab(ch, 0));
    let asked = std::cell::RefCell::new(Vec::new());
    resolve_with(&mut g, |g| {
        asked.borrow_mut().push(g.kind().unwrap());
        match g.kind() {
            Some(DecisionKind::YesNo) => Some(Choice::Yes),
            Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
            Some(DecisionKind::Sacrifice) => Some(Choice::Card(ring)),
            Some(DecisionKind::Target) => Some(player(1)),
            Some(DecisionKind::ChooseExiled) => Some(Choice::Pass),
            _ => None,
        }
    });
    let crab = g.card(0, "Crabomination", Zone::Battlefield);
    assert!(asked.borrow().contains(&DecisionKind::ChooseAltCost), "{:?}", asked.borrow());
    assert_eq!(g.obj(ring).zone, Zone::Graveyard, "emerged\n{}", log(&g));
    let _ = crab;
}

#[test]
fn a_dashed_creature_is_cast_counterable_and_at_sorcery_speed_cr_702_109a() {
    // ruling: 4f0780ad
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Ragavan, Nimble Pilferer"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let r = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    cast_recording(&mut g, Choice::Cast(r), |g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(1)));
    assert_eq!(g.obj(r).zone, Zone::Stack, "on the stack");
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    let t = g.tgt(r);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(t));
    assert_eq!(g.obj(r).zone, Zone::Graveyard, "countered\n{}", log(&g));
    // Not on P1's turn or with a spell on the stack.
    let g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &["Ragavan, Nimble Pilferer"]).at(Step::Upkeep, 1).start();
    let r = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(r)));
}

// ---- flashback (CR 702.34) ---------------------------------------------------------------------------

/// P0 casts Snapcaster Mage (paid with Islands) targeting `target` in
/// its graveyard.
fn snap(g: &mut Game, target: CardIdx) {
    let s = g.card(0, "Snapcaster Mage", Zone::Hand);
    g.choose(Choice::Cast(s));
    let t = g.tgt(target);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) if g.has_option(t) => Some(t),
        Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| matches!(c, Choice::Card(x) | Choice::Ability(x, _) if g.name(x) == "Island")),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Snapcaster Mage", Zone::Battlefield), 1, "{}", log(g));
}

#[test]
fn flashback_costs_snapcasters_and_printed_x_kicker_no_overload_additional_costs_cr_702_34a_118_9() {
    // ruling: a92b37a5 d38450c1 40dba187 e8a86435 0a2cf1a9
    // Snapcaster on Faithless Looting: two flashback costs, {R} or {2}{R}:
    // castable with one Mountain left. On Winds of Abandon: no overload. On
    // Burst Lightning: kicker offered. On Tinker: an artifact sacrificed. On
    // Mind Twist: X chosen and paid. On Treasure Cruise: delve with it.
    let snap_scen = |gy: &[&str], lands: &[&str], extra: &[&str]| {
        scen()
            .battlefield(0, &["Island", "Island"])
            .battlefield(0, lands)
            .battlefield(0, extra)
            .hand(0, &["Snapcaster Mage"])
            .graveyard(0, gy)
            .battlefield(1, &["Test Bear"])
            .hand(1, &["Island", "Island", "Island"])
            .at(Step::Main1, 0)
            .start()
    };
    let mut g = snap_scen(&["Faithless Looting"], &["Mountain"], &[]);
    let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
    snap(&mut g, fl);
    assert!(g.has_option(Choice::Cast(fl)), "{{R}} with one Mountain: {}", opts(&g));
    g.choose(Choice::Cast(fl));
    resolve(&mut g);
    assert_eq!(g.obj(fl).zone, Zone::Exile, "{}", log(&g));
    // With three Mountains both flashback costs can be paid: P0 chooses.
    let mut g = snap_scen(&["Faithless Looting"], &["Mountain", "Mountain", "Mountain"], &[]);
    let fl = g.card(0, "Faithless Looting", Zone::Graveyard);
    snap(&mut g, fl);
    g.choose(Choice::Cast(fl));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseRoute), "two flashback costs: {}", opts(&g));
    assert_eq!(g.pending_decision().unwrap().options.len(), 2, "{}", opts(&g));
    // Winds of Abandon.
    let mut g = snap_scen(&["Winds of Abandon"], &["Plains", "Plains", "Plains", "Plains", "Plains", "Plains"], &[]);
    let w = g.card(0, "Winds of Abandon", Zone::Graveyard);
    snap(&mut g, w);
    let asked = cast_recording(&mut g, Choice::Cast(w), |_| None);
    assert!(!asked.contains(&DecisionKind::ChooseAltCost), "no overload with flashback: {asked:?}");
    // Burst Lightning kicked.
    let mut g = snap_scen(&["Burst Lightning"], &["Mountain", "Mountain", "Mountain", "Mountain", "Mountain"], &[]);
    let bl = g.card(0, "Burst Lightning", Zone::Graveyard);
    snap(&mut g, bl);
    let asked = cast_recording(&mut g, Choice::Cast(bl), |g| match g.kind() {
        Some(DecisionKind::ChooseCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert!(asked.contains(&DecisionKind::ChooseCost), "kicker offered: {asked:?}");
    resolve(&mut g);
    assert_eq!(g.life(1), 16, "kicked: 4\n{}", log(&g));
    // Tinker.
    let mut g = snap_scen(&["Tinker"], &["Island", "Island", "Island"], &["Mind Stone"]);
    let t = g.card(0, "Tinker", Zone::Graveyard);
    snap(&mut g, t);
    cast_recording(&mut g, Choice::Cast(t), |_| None);
    assert_eq!(g.count_in(0, "Mind Stone", Zone::Graveyard), 1, "sacrificed\n{}", log(&g));
    // Mind Twist, X = 2.
    let mut g = snap_scen(&["Mind Twist"], &["Swamp", "Swamp", "Swamp"], &[]);
    let mt = g.card(0, "Mind Twist", Zone::Graveyard);
    snap(&mut g, mt);
    let asked = cast_recording(&mut g, Choice::Cast(mt), |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(2)),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert!(asked.contains(&DecisionKind::ChooseMode), "X chosen: {asked:?}");
    assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 5, "{{2}}{{B}} paid (and Snapcaster's {{1}}{{U}})\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.player(1).hand.len(), 1, "two discarded");
    // Treasure Cruise: delve.
    let mut gy = vec!["Treasure Cruise"];
    gy.extend(["Island"; 7]);
    let mut g = snap_scen(&gy, &["Island"], &[]);
    let tc = g.card(0, "Treasure Cruise", Zone::Graveyard);
    snap(&mut g, tc);
    assert!(g.has_option(Choice::Cast(tc)), "{{7}}{{U}}: one Island and seven cards: {}", opts(&g));
    cast_recording(&mut g, Choice::Cast(tc), |g| (g.kind() == Some(DecisionKind::ChooseGraveyardCard)).then(|| first_non_pass(g)).flatten());
    resolve(&mut g);
    assert_eq!(g.obj(tc).zone, Zone::Exile);
    assert_eq!(g.player(0).hand.len(), 3, "{}", log(&g));
}

// ---- cost increases on alternative costs (CR 601.2f) -------------------------------------------------

#[test]
fn thalias_tax_applies_to_pyrokinesis_and_baleful_masterys_alternative_costs_cr_601_2f() {
    // ruling: 6cfaf27f 084701b5
    // With Thalia: Pyrokinesis for a red card needs {1} too. Baleful
    // Mastery for {1}{B} costs {2}{B}, and the {1}{B} still counts as paid:
    // P1 draws.
    for (lands, castable) in [(&[][..], false), (&["Mountain"][..], true)] {
        let g = scen().battlefield(0, &["Thalia, Guardian of Thraben"]).battlefield(0, lands).hand(0, &["Pyrokinesis", "Lightning Bolt"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
        let p = g.card(0, "Pyrokinesis", Zone::Hand);
        assert_eq!(g.has_option(Choice::Cast(p)), castable, "{lands:?}: {}", opts(&g));
    }
    let mut g = scen()
        .battlefield(0, &["Thalia, Guardian of Thraben", "Swamp", "Swamp", "Swamp"])
        .hand(0, &["Baleful Mastery"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bm = g.card(0, "Baleful Mastery", Zone::Hand);
    let bear = bf(&g, 1, "Test Bear");
    let hand1 = g.player(1).hand.len();
    let tb = g.tgt(bear);
    cast_recording(&mut g, Choice::Cast(bm), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
        Some(DecisionKind::Target) => Some(tb),
        _ => None,
    });
    assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 3, "{{1}}{{B}} and Thalia's {{1}}\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.player(1).hand.len(), hand1 + 1, "the {{1}}{{B}} cost was paid: P1 draws\n{}", log(&g));
}

// ---- escape (CR 702.138) ------------------------------------------------------------------------------

#[test]
fn escape_exiles_the_card_discarded_for_its_additional_cost_and_casts_a_split_half_cr_702_138_709_3() {
    // ruling: bea302d9 6257ac75
    // Underworld Breach: Bitter Triumph with two other cards in the
    // graveyard; its additional cost discards P0's last card, which is then
    // exiled as the third card for escape. Tinker likewise with the artifact
    // it sacrifices. Life // Death: Death escaped ({1}{B} and three cards),
    // returning a Test Bear.
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Swamp", "Swamp"])
        .graveyard(0, &["Bitter Triumph", "Island", "Island"])
        .hand(0, &["Forest"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bt = g.card(0, "Bitter Triumph", Zone::Graveyard);
    let bear = bf(&g, 1, "Test Bear");
    let forest = g.card(0, "Forest", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bt)), "the discarded card can pay escape: {}", opts(&g));
    let tb = g.tgt(bear);
    cast_recording(&mut g, Choice::Cast(bt), |g| match g.kind() {
        Some(DecisionKind::Target) => Some(tb),
        _ if g.has_option(Choice::Card(forest)) => Some(Choice::Card(forest)),
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.life(0), 20, "a card discarded, not 3 life");
    assert_eq!(g.obj(forest).zone, Zone::Exile, "the discarded Forest exiled for escape\n{}", log(&g));
    // Tinker: the artifact it sacrifices is the third card exiled.
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Island", "Island", "Island", "Mind Stone"])
        .graveyard(0, &["Tinker", "Island", "Island"])
        .library(0, &["Sol Ring", "Island"])
        .at(Step::Main1, 0)
        .start();
    let t = g.card(0, "Tinker", Zone::Graveyard);
    let ms = bf(&g, 0, "Mind Stone");
    assert!(g.has_option(Choice::Cast(t)), "the sacrificed card can pay escape: {}", opts(&g));
    let ring = g.card(0, "Sol Ring", Zone::Library);
    let r = Choice::Def(g.obj(ring).def);
    cast_recording(&mut g, Choice::Cast(t), |g| match g.kind() {
        Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| !matches!(c, Choice::Card(x) | Choice::Ability(x, _) if x == ms)),
        _ => None,
    });
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::SearchLibrary)).then_some(r));
    assert_eq!(g.obj(ms).zone, Zone::Exile, "Mind Stone sacrificed, then exiled for escape\n{}", log(&g));
    assert_eq!(g.obj(ring).zone, Zone::Battlefield, "{}", log(&g));
    // Life // Death: Death.
    let mut g = scen()
        .battlefield(0, &["Underworld Breach", "Swamp", "Swamp"])
        .graveyard(0, &["Life // Death", "Test Bear", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let ld = g.card(0, "Life // Death", Zone::Graveyard);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    let ch = g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&o| matches!(o, Choice::Cast(x) | Choice::CastFace(x) | Choice::Faces(x, _) if x == ld));
    let ch = ch.unwrap_or_else(|| panic!("escape offered: {}", opts(&g)));
    let tb = g.tgt(bear);
    cast_recording(&mut g, ch, |g| match g.kind() {
        Some(DecisionKind::ChooseFace) => {
            let d = g.pending_decision().unwrap();
            d.options.iter().map(|o| o.choice).last()
        }
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        Some(DecisionKind::ChooseGraveyardCard) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| c != Choice::Card(bear) && c != Choice::Pass),
        _ => None,
    });
    resolve(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "Death escaped\n{}", log(&g));
    assert_eq!(g.life(0), 18, "Death: the Bear's mana value 2");
    assert_eq!(g.exile_zone().len(), 3, "three other cards exiled");
}

// ---- miracle (CR 702.94) --------------------------------------------------------------------------------

#[test]
fn a_spell_cast_for_its_miracle_cost_keeps_its_mana_value_cr_702_94a_202_3() {
    // ruling: d3edc491
    // Metamorphosis Fanatic cast for {1}{B} as its miracle; P1 Mana Drains
    // it: {C}{C}{C}{C}{C}{C}.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Metamorphosis Fanatic", "Island"])
        .library(1, &["Island"; 5])
        .battlefield(0, &["Swamp", "Swamp"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Mana Drain"])
        .at(Step::Upkeep, 0)
        .start();
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    let m = g.card(0, "Metamorphosis Fanatic", Zone::Hand);
    g.choose(Choice::Yes);
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    pay(&mut g);
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 1 && g.obj(m).zone == Zone::Stack);
    let md = g.card(1, "Mana Drain", Zone::Hand);
    g.choose(Choice::Cast(md));
    let t = g.tgt(m);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(t)).then_some(t));
    assert_eq!(g.obj(m).zone, Zone::Graveyard, "{}", log(&g));
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    resolve(&mut g);
    assert_eq!(g.player(1).pool.amt[5], 6, "{}", log(&g));
}

// ---- devotion, domain, converge (CR 700.5, 702.149, 702.140) -----------------------------------

#[test]
fn devotion_counts_the_mana_costs_colored_hybrid_and_phyrexian_symbols_as_the_ability_resolves_cr_700_5() {
    // ruling: 2508542e 4760ea2e a40c5a6d e36f6dac
    // Thassa's Oracle ({U}{U}), Lutri ({1}{U/R}{U/R}: hybrid), Phyrexian
    // Metamorph ({3}{U/P}: Phyrexian), Urza ({2}{U}{U}; "Add {U}" in its
    // text doesn't count), Sol Ring ({1}: none): devotion to blue 7. Seven
    // cards in the library: P0 wins; eight: not. Six cards, and P1 Bolts
    // the Oracle in response: devotion 5 as the trigger resolves, no win.
    let base = |lib: usize, bolt: bool| {
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &vec!["Island"; lib])
            .library(1, &["Forest"; 5])
            .battlefield(0, &["Lutri, the Spellchaser", "Phyrexian Metamorph", "Urza, Lord High Artificer", "Sol Ring", "Island", "Island"])
            // (The Metamorph copies nothing here: a 0/0 kept alive.)
            .counters(0, "Phyrexian Metamorph", Counter::P1P1, 2)
            .hand(0, &["Thassa's Oracle"])
            .battlefield(1, &["Mountain"])
            .hand(1, if bolt { &["Lightning Bolt"][..] } else { &[][..] })
            .at(Step::Main1, 0)
            .start();
        let o = g.card(0, "Thassa's Oracle", Zone::Hand);
        g.choose(Choice::Cast(o));
        if bolt {
            quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 1 && g.obj(o).zone == Zone::Battlefield && !g.stack().is_empty());
            cast_at(&mut g, 1, "Lightning Bolt", o);
        } else {
            resolve(&mut g);
        }
        g
    };
    assert_eq!(base(7, false).outcome().and_then(|o| o.winner), Some(0), "devotion 7, seven cards");
    let g = base(8, false);
    assert!(!g.is_over(), "eight cards: no win\n{}", log(&g));
    let g = base(6, true);
    assert_eq!(g.count_in(0, "Thassa's Oracle", Zone::Graveyard), 1, "{}", log(&g));
    assert!(!g.is_over(), "the Oracle gone: devotion 5 < 6\n{}", log(&g));
    let g = base(5, true);
    assert_eq!(g.outcome().and_then(|o| o.winner), Some(0), "devotion 5, five cards: still a win\n{}", log(&g));
}

#[test]
fn domain_counts_basic_land_types_not_lands_cr_702_140() {
    // ruling: 3a28a872 59274c44
    // Territorial Kavu with Watery Grave (Island Swamp), an Island and
    // Urza's Saga (Urza's, Saga: not basic land types): 2/2. Adding a
    // Plains and a Mountain: 4/4.
    let g = scen().battlefield(0, &["Territorial Kavu", "Watery Grave", "Island", "Urza's Saga"]).at(Step::Main1, 0).start();
    let k = bf(&g, 0, "Territorial Kavu");
    assert_eq!(g.pt(k), (2, 2), "Island and Swamp");
    let g = scen().battlefield(0, &["Territorial Kavu", "Watery Grave", "Island", "Plains", "Mountain", "Urza's Saga"]).at(Step::Main1, 0).start();
    let k = bf(&g, 0, "Territorial Kavu");
    assert_eq!(g.pt(k), (4, 4));
}

#[test]
fn converge_counts_mana_spent_on_increases_and_mana_value_is_checked_on_resolution_cr_702_149_601_2f() {
    // ruling: a95c3d78 51285906
    // Thalia taxes Prismatic Ending {1}: X = 1, {W}, the tax, paid with a
    // Plains, an Island and a Swamp: three colours spent, P1's Pinnacle
    // Emissary (mana value 3) is exiled. Without Thalia, X = 1 paid with
    // two colours: the Emissary is still a legal target (mana value isn't
    // a targeting condition), checked on resolution: it stays.
    let run = |thalia: bool| {
        let mut p0 = vec!["Plains", "Island", "Swamp"];
        if thalia {
            p0.push("Thalia, Guardian of Thraben");
        }
        let mut g = scen().battlefield(0, &p0).hand(0, &["Prismatic Ending"]).battlefield(1, &["Pinnacle Emissary"]).at(Step::Main1, 0).start();
        let pe = g.card(0, "Prismatic Ending", Zone::Hand);
        let em = bf(&g, 1, "Pinnacle Emissary");
        g.choose(Choice::Cast(pe));
        let t = g.tgt(em);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::ChooseMode) => Some(Choice::Number(1)),
            Some(DecisionKind::Target) => {
                assert!(g.has_option(t), "a legal target whatever its mana value: {}", opts(g));
                Some(t)
            }
            _ => None,
        });
        (g, em)
    };
    let (g, em) = run(true);
    assert_eq!(g.obj(em).zone, Zone::Exile, "three colours spent\n{}", log(&g));
    let (g, em) = run(false);
    assert_eq!(g.obj(em).zone, Zone::Battlefield, "two colours: mana value 3 too high\n{}", log(&g));
}

// ---- mana abilities that make no mana; what mana can pay (CR 106.6, 605) -----------------------

#[test]
fn chrome_mox_makes_one_mana_of_an_exiled_cards_colours_never_colorless_cr_106_1b_702_114() {
    // ruling: 8827464d 5e6e8e38 78edf710 9924d9da 747e785b 729bf672
    // Imprinting Sowing Mycospawn (devoid: colorless in hand too, though
    // {G} is in its mana cost): no mana at all, not {C}. Vindicate (white
    // and black): {W} or {B}, one mana. Gitaxian Probe ({U/P}): blue,
    // however it would be paid.
    let imprint = |card: &str| {
        let mut g = scen().hand(0, &["Chrome Mox", card]).at(Step::Main1, 0).start();
        let m = g.card(0, "Chrome Mox", Zone::Hand);
        let c = g.card(0, card, Zone::Hand);
        g.choose(Choice::Cast(m));
        resolve_with(&mut g, |g| g.has_option(Choice::Card(c)).then_some(Choice::Card(c)));
        assert_eq!(g.obj(c).zone, Zone::Exile, "{card}\n{}", log(&g));
        g
    };
    let g = imprint("Sowing Mycospawn");
    assert!(!can_pay(&g, 0, "{1}") && !can_pay(&g, 0, "{C}") && !can_pay(&g, 0, "{G}"), "devoid: colorless, no mana");
    let g = imprint("Vindicate");
    assert!(can_pay(&g, 0, "{W}") && can_pay(&g, 0, "{B}"), "either colour");
    assert!(!can_pay(&g, 0, "{W}{B}") && !can_pay(&g, 0, "{C}"), "one mana, never colorless");
    let g = imprint("Gitaxian Probe");
    assert!(can_pay(&g, 0, "{U}"), "Gitaxian Probe is blue");
}

#[test]
fn mox_amber_makes_one_mana_of_one_legends_colour_none_from_colorless_legends_cr_106_1b() {
    // ruling: 7376b740 1b60b365
    // Golos (a colorless legendary creature): no mana. Thalia (white) and
    // Emry (blue): {W} or {U}, not both.
    let g = scen().battlefield(0, &["Mox Amber", "Golos, Tireless Pilgrim"]).at(Step::Main1, 0).start();
    assert!(!can_pay(&g, 0, "{1}"), "colorless legends: no mana");
    let g = scen().battlefield(0, &["Mox Amber", "Thalia, Guardian of Thraben", "Emry, Lurker of the Loch"]).at(Step::Main1, 0).start();
    assert!(can_pay(&g, 0, "{W}") && can_pay(&g, 0, "{U}"));
    assert!(!can_pay(&g, 0, "{W}{U}"), "one mana, not one of each");
}

#[test]
fn squandered_resources_gets_no_mana_from_a_land_that_makes_none_cr_605_1a() {
    // ruling: a755e0f5
    // Urza's Saga with no chapter ability yet (no mana ability): sacrificed
    // to Squandered Resources it adds nothing; a Swamp's sacrifice adds {B}.
    let g = scen().battlefield(0, &["Squandered Resources", "Urza's Saga"]).at(Step::Main1, 0).start();
    assert!(!can_pay(&g, 0, "{1}"), "no mana");
    let g = scen().battlefield(0, &["Squandered Resources", "Urza's Saga", "Swamp"]).at(Step::Main1, 0).start();
    assert!(can_pay(&g, 0, "{B}{B}"), "the Swamp tapped and sacrificed");
    assert!(!can_pay(&g, 0, "{B}{B}{1}"));
}

#[test]
fn thopter_foundry_may_sacrifice_itself_to_its_own_ability_cr_602_2() {
    // ruling: 47a83114
    let mut g = scen().battlefield(0, &["Thopter Foundry", "Island"]).at(Step::Main1, 0).start();
    let tf = bf(&g, 0, "Thopter Foundry");
    assert!(g.has_option(ab(tf, 0)), "{}", opts(&g));
    g.choose(ab(tf, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(tf)).then_some(Choice::Card(tf)));
    assert_eq!(g.obj(tf).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(tokens_named(&g, "Thopter").len(), 1);
    assert_eq!(g.life(0), 21);
}

#[test]
fn mishras_workshop_mana_cant_pay_a_cost_imposed_after_the_cast_cr_106_6() {
    // ruling: 5260a564
    // Voltaic Key cast with the Workshop's {C}{C}{C} ({C}{C} left, for
    // artifact spells only); P1's Daze: P0 can't pay {1} with it.
    let mut g = scen()
        .battlefield(0, &["Mishra's Workshop"])
        .hand(0, &["Voltaic Key"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    let key = g.card(0, "Voltaic Key", Zone::Hand);
    g.choose(Choice::Cast(key));
    pay(&mut g);
    g.drive_until(|g| at_priority(g) && g.turn().priority == 1);
    assert_eq!(g.player(0).pool.amt[5], 2, "{}", log(&g));
    let d = g.card(1, "Daze", Zone::Hand);
    g.choose(Choice::Cast(d));
    let t = g.tgt(key);
    let offered = std::cell::Cell::new(false);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(0)),
        Some(DecisionKind::Target) => Some(t),
        Some(DecisionKind::YesNo) if g.pending_decision().unwrap().player == 0 => {
            offered.set(g.has_option(Choice::Yes));
            None
        }
        _ => None,
    });
    assert!(!offered.get(), "the Workshop's mana can't pay Daze's {{1}}\n{}", log(&g));
    assert_eq!(g.obj(key).zone, Zone::Graveyard, "countered\n{}", log(&g));
}

#[test]
fn urzas_saga_finds_a_mana_cost_of_0_or_1_not_a_mana_value_cr_714_202_1() {
    // ruling: c8a74bc2
    // Chapter III: Sol Ring ({1}) and Mox Pearl ({0}), not Walking Ballista
    // ({X}{X}, mana value 0) or Sewer-veillance Cam ({U}, mana value 1).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Sol Ring", "Mox Pearl", "Walking Ballista", "Sewer-veillance Cam", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Urza's Saga"])
        .counters(0, "Urza's Saga", Counter::Lore, 2)
        .at(Step::Untap, 0)
        .start();
    let found = std::cell::RefCell::new(Vec::new());
    quiet_until_with(&mut g, |g| g.turn().step == Step::Main1 && g.stack().is_empty() && at_priority(g), |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => {
            let mut v: Vec<String> = g.pending_decision().unwrap().options.iter().filter_map(|o| match o.choice {
                Choice::Def(d) => Some(db().def(d).name.to_string()),
                _ => None,
            }).collect();
            v.sort();
            *found.borrow_mut() = v;
            Some(Choice::Pass)
        }
        _ => None,
    });
    assert_eq!(*found.borrow(), vec!["Mox Pearl".to_string(), "Sol Ring".to_string()], "{}", log(&g));
}

#[test]
fn affinity_reduces_only_generic_mana_cr_702_41a_601_2f() {
    // ruling: 8f8116e7
    // Emry ({2}{U}) with Sol Ring, Mind Stone and Mox Jet: {U} still; no
    // blue source, not castable; with an Island, the Island alone pays.
    let g = scen().battlefield(0, &["Sol Ring", "Mind Stone", "Mox Jet"]).hand(0, &["Emry, Lurker of the Loch"]).at(Step::Main1, 0).start();
    let e = g.card(0, "Emry, Lurker of the Loch", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(e)), "the {{U}} isn't reduced: {}", opts(&g));
    let mut g = scen().battlefield(0, &["Sol Ring", "Mind Stone", "Mox Jet", "Island"]).hand(0, &["Emry, Lurker of the Loch"]).at(Step::Main1, 0).start();
    let e = g.card(0, "Emry, Lurker of the Loch", Zone::Hand);
    g.choose(Choice::Cast(e));
    pay(&mut g);
    assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 1, "only {{U}}\n{}", log(&g));
}

#[test]
fn rain_of_filth_grants_its_ability_only_to_lands_controlled_as_it_resolves_cr_611_2c() {
    // ruling: 380a3828
    // Rain of Filth paid with a Swamp; then P0 plays an Island: the Island
    // can't be sacrificed for {B}.
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Rain of Filth", "Island"]).at(Step::Main1, 0).start();
    let r = g.card(0, "Rain of Filth", Zone::Hand);
    g.choose(Choice::Cast(r));
    resolve(&mut g);
    let i = g.card(0, "Island", Zone::Hand);
    g.choose(Choice::Play(i));
    resolve(&mut g);
    assert!(can_pay(&g, 0, "{B}{U}"), "the Swamp's sacrifice and the Island's tap");
    assert!(!can_pay(&g, 0, "{B}{B}"), "the Island has no sacrifice ability");
}

#[test]
fn arwen_with_lethal_damage_dies_once_her_counter_is_removed_as_the_cost_cr_602_2_704_5g() {
    // ruling: 252cd557
    // Arwen (2/2, indestructible counter) Bolted survives; she removes the
    // counter to activate her ability: destroyed before it resolves (no
    // counters on her); the Test Bear still gets indestructible and its
    // counters.
    let mut g = scen()
        .battlefield(0, &["Arwen, Mortal Queen", "Test Bear", "Forest"])
        .counters(0, "Arwen, Mortal Queen", Counter::Kw(Kw::Indestructible), 1)
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let ar = bf(&g, 0, "Arwen, Mortal Queen");
    let bear = bf(&g, 0, "Test Bear");
    cast_at(&mut g, 1, "Lightning Bolt", ar);
    assert_eq!(g.obj(ar).zone, Zone::Battlefield, "indestructible");
    quiet_until(&mut g, |g| at_priority(g) && g.turn().priority == 0);
    assert_eq!(g.turn().active, 1, "still P1's turn: the damage is marked");
    g.choose(ab(ar, 0));
    let t = g.tgt(bear);
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(t);
    }
    pay(&mut g);
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.obj(ar).zone, Zone::Graveyard, "lethal damage, no longer indestructible\n{}", log(&g));
    resolve(&mut g);
    assert!(g.has_kw(bear, Kw::Indestructible), "{}", log(&g));
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1);
}

#[test]
fn values_an_ability_counts_are_counted_once_as_it_resolves_cr_608_2h() {
    // ruling: 98855f29 4580d03c 4fb42bfe
    // Bloodtithe Harvester with one Blood token: -2/-2 to P1's Test Giant;
    // a second Blood token made after it resolved doesn't change it.
    // Consult the Star Charts and Pillage the Bog with four lands: P1
    // destroys one with Wasteland in response; X is counted as they
    // resolve (three lands: three cards looked at, six for Pillage).
    let mut g = scen()
        .battlefield(0, &["Bloodtithe Harvester", "Swamp", "Mountain", "Swamp", "Mountain"])
        .hand(0, &["Bloodtithe Harvester", "Bloodtithe Harvester"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let bh = bf(&g, 0, "Bloodtithe Harvester");
    let giant = bf(&g, 1, "Test Giant");
    // (The battlefield Harvester didn't enter: one Blood from a second.)
    let h2 = g.card(0, "Bloodtithe Harvester", Zone::Hand);
    g.choose(Choice::Cast(h2));
    resolve(&mut g);
    assert_eq!(tokens_named(&g, "Blood").len(), 1);
    g.choose(ab(bh, 0));
    resolve_at(&mut g, giant);
    assert_eq!(g.pt(giant), (3, 3), "-2/-2: one Blood token\n{}", log(&g));
    let h3 = g.card(0, "Bloodtithe Harvester", Zone::Hand);
    g.choose(Choice::Cast(h3));
    resolve(&mut g);
    assert_eq!(tokens_named(&g, "Blood").len(), 2);
    assert_eq!(g.pt(giant), (3, 3), "counted once: still -2/-2");
    // Consult the Star Charts and Pillage the Bog.
    let lib = ["Sol Ring", "Mox Pearl", "Mox Jet", "Mox Ruby", "Mox Emerald", "Mox Sapphire", "Lotus Petal", "Black Lotus", "Mana Crypt", "Mana Vault"];
    for (spell, per_land) in [("Consult the Star Charts", 1), ("Pillage the Bog", 2)] {
        let mut g = Scenario::new(db())
            .spec(|s| s.auto_single = false)
            .library(0, &lib)
            .library(1, &["Forest"; 5])
            .battlefield(0, &["Watery Grave", "Overgrown Tomb", "Island", "Swamp", "Forest"])
            .hand(0, &[spell])
            .battlefield(1, &["Wasteland"])
            .at(Step::Main1, 0)
            .start();
        let c = g.card(0, spell, Zone::Hand);
        let wg = bf(&g, 0, "Watery Grave");
        let ot = bf(&g, 0, "Overgrown Tomb");
        g.choose(Choice::Cast(c));
        for _ in 0..20 {
            if at_priority(&g) && g.turn().priority == 1 {
                break;
            }
            if at_priority(&g) {
                g.pass();
                continue;
            }
            let ch = match g.kind() {
                Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&ch| !matches!(ch, Choice::Card(x) | Choice::Ability(x, _) if x == wg || x == ot)).unwrap(),
                _ => g.pending_decision().unwrap().options[0].choice,
            };
            g.choose(ch);
        }
        assert_eq!(g.permanents_of(0).filter(|&x| g.obj(x).tapped).count(), 2, "{spell}: two basics paid\n{}", log(&g));
        let w = bf(&g, 1, "Wasteland");
        let tw = g.tgt(wg);
        g.choose(ab(w, 1));
        let looked = std::cell::Cell::new(0usize);
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) if g.has_option(tw) => Some(tw),
            Some(DecisionKind::PutInHand) => {
                let n = g.pending_decision().unwrap().options.iter().filter(|o| o.choice != Choice::Pass).count();
                looked.set(looked.get().max(n));
                None
            }
            _ => None,
        });
        assert_eq!(g.obj(wg).zone, Zone::Graveyard, "{spell}\n{}", log(&g));
        assert_eq!(looked.get(), 4 * per_land, "{spell}: the lands as it resolves: four left\n{}", log(&g));
    }
}

#[test]
fn a_tokens_mana_value_is_zero_unless_it_copies_something_cr_202_3d_707_2() {
    // ruling: 2080ede6
    // P1 has a Clue (mana value 0) and Reflection of Kiki-Jiki's token copy
    // of a Test Trampler (4): P0's Abrupt Decay ("mana value 3 or less")
    // can target the Clue, not the copy.
    let mut g = scen()
        .battlefield(1, &["Fable of the Mirror-Breaker // Reflection of Kiki-Jiki", "Test Trampler", "Mountain", "Plains"])
        .transformed(1, "Fable of the Mirror-Breaker // Reflection of Kiki-Jiki")
        .hand(1, &["Thraben Inspector"])
        .battlefield(0, &["Swamp", "Forest"])
        .hand(0, &["Abrupt Decay"])
        .at(Step::Main1, 1)
        .start();
    let r = bf(&g, 1, "Reflection of Kiki-Jiki");
    let tr = bf(&g, 1, "Test Trampler");
    g.choose(ab(r, 0));
    resolve_at(&mut g, tr);
    let ti = g.card(1, "Thraben Inspector", Zone::Hand);
    g.choose(Choice::Cast(ti));
    resolve(&mut g);
    let copy = tokens_named(&g, "Test Trampler")[0];
    let clue = tokens_named(&g, "Clue")[0];
    g.pass();
    assert!(at_priority(&g) && g.turn().priority == 0, "{}", log(&g));
    let d = g.card(0, "Abrupt Decay", Zone::Hand);
    g.choose(Choice::Cast(d));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(clue)), "the Clue: mana value 0: {}", opts(&g));
    assert!(!g.has_option(g.tgt(copy)), "the copy: mana value 4: {}", opts(&g));
    assert!(!g.has_option(g.tgt(tr)), "the Trampler: 4");
}

// ---- emerge (CR 702.119) ---------------------------------------------------------------------------

#[test]
fn emerge_reduces_by_the_sacrificed_artifacts_printed_mana_value_never_the_colored_part_cr_702_119() {
    // ruling: 2e4566d6 62210dab 936c4c1c dfea8aa9 d5867659
    // Crabomination's emerge {5}{B}{B}: sacrificing a Walking Ballista cast
    // with X = 2 (mana value 0) or a Clue (0) reduces nothing: seven mana.
    // Sacrificing Golos (5): {B}{B} only; Bolas's Citadel (6): still
    // {B}{B}. Sacrificing Sol Ring after tapping it for {C}{C}: four lands
    // and the Ring's mana pay {4}{B}{B} (it is still there as the mana is
    // paid).
    let emerge = |sac: &str, lands: usize| {
        let mut g = scen()
            .battlefield(0, &vec!["Swamp"; lands])
            .battlefield(0, &[sac])
            .hand(0, &["Crabomination"])
            .library(1, &["Island", "Forest"])
            .hand(1, &["Forest"])
            .at(Step::Main1, 0)
            .start();
        let s = bf(&g, 0, sac);
        let c = g.card(0, "Crabomination", Zone::Hand);
        let offered = g.has_option(Choice::Cast(c));
        if offered {
            g.choose(Choice::Cast(c));
            resolve_with(&mut g, |g| match g.kind() {
                Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
                Some(DecisionKind::Sacrifice) if g.has_option(Choice::Card(s)) => Some(Choice::Card(s)),
                Some(DecisionKind::Target) => Some(player(1)),
                Some(DecisionKind::ChooseExiled) => Some(Choice::Pass),
                _ => None,
            });
        }
        (g, s, offered)
    };
    for (sac, lands, ok) in [("Golos, Tireless Pilgrim", 2, true), ("Golos, Tireless Pilgrim", 1, false), ("Bolas's Citadel", 2, true), ("Sol Ring", 4, true), ("Sol Ring", 3, false)] {
        let (g, s, offered) = emerge(sac, lands);
        assert_eq!(offered, ok, "{sac} with {lands} Swamps: {}", opts(&g));
        if ok {
            assert_eq!(g.obj(s).zone, Zone::Graveyard, "{sac} sacrificed\n{}", log(&g));
            assert_eq!(g.count_in(0, "Crabomination", Zone::Battlefield), 1, "{}", log(&g));
        }
    }
    // A Walking Ballista cast with X = 2, and a Clue: mana value 0.
    let mut g = scen()
        .battlefield(0, &["Swamp"; 7])
        .battlefield(0, &["Island", "Island", "Island", "Island", "Plains"])
        .hand(0, &["Walking Ballista", "Crabomination", "Thraben Inspector"])
        .library(1, &["Island", "Forest"])
        .hand(1, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let wb = g.card(0, "Walking Ballista", Zone::Hand);
    cast_x(&mut g, wb, 2);
    resolve(&mut g);
    let ti = g.card(0, "Thraben Inspector", Zone::Hand);
    g.choose(Choice::Cast(ti));
    resolve(&mut g);
    let clue = tokens_named(&g, "Clue")[0];
    let c = g.card(0, "Crabomination", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    assert!(g.has_option(Choice::Card(wb)) && g.has_option(Choice::Card(clue)), "{}", opts(&g));
    let untapped = g.permanents_of(0).filter(|&x| !g.obj(x).tapped).count();
    g.choose(Choice::Card(clue));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseExiled) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(untapped - g.permanents_of(0).filter(|&x| !g.obj(x).tapped).count(), 7, "the full {{5}}{{B}}{{B}}: seven lands\n{}", log(&g));
}

// ---- Arena of Glory's haste mana (CR 106.6) ---------------------------------------------------------

#[test]
fn arena_of_glory_mana_gives_haste_to_creature_spells_only_cr_106_6() {
    // ruling: 969ce586 9f8d1cca ce428cbf f7ca85a0
    // Arena's {R}{R} on Smuggler's Copter (an artifact spell crewed later):
    // no haste; on Lingering Souls: its Spirits get none; on Thieving
    // Skydiver's kicker {1}: haste; split over Ragavan and a Test Dwarf:
    // both have haste.
    let arena = |hand: &[&str], extra: &[&str]| {
        scen()
            .battlefield(0, &["Arena of Glory", "Mountain"])
            .battlefield(0, extra)
            .hand(0, hand)
            .at(Step::Main1, 0)
            .start()
    };
    /// Cast `c`, paying with Arena of Glory's exert ability first.
    fn cast_with_arena(g: &mut Game, c: CardIdx, answer: impl Fn(&Game) -> Option<Choice>) {
        let a = bf(g, 0, "Arena of Glory");
        g.choose(Choice::Cast(c));
        resolve_with(g, |g| match g.kind() {
            Some(DecisionKind::PayMana) if g.has_option(ab(a, 1)) => Some(ab(a, 1)),
            _ => answer(g),
        });
    }
    // Smuggler's Copter.
    let mut g = arena(&["Smuggler's Copter"], &["Test Bear"]);
    let sc = g.card(0, "Smuggler's Copter", Zone::Hand);
    cast_with_arena(&mut g, sc, |_| None);
    assert!(g.obj(bf(&g, 0, "Arena of Glory")).tapped);
    let bear = bf(&g, 0, "Test Bear");
    g.choose(ab(sc, 0));
    resolve_with(&mut g, |g| g.has_option(Choice::Card(bear)).then_some(Choice::Card(bear)));
    assert!(g.is_creature(sc) && !g.has_kw(sc, Kw::Haste), "a noncreature spell: no haste\n{}", log(&g));
    // Lingering Souls.
    let mut g = arena(&["Lingering Souls"], &["Plains"]);
    let ls = g.card(0, "Lingering Souls", Zone::Hand);
    cast_with_arena(&mut g, ls, |_| None);
    let sp = tokens_named(&g, "Spirit");
    assert_eq!(sp.len(), 2, "{}", log(&g));
    assert!(sp.iter().all(|&t| !g.has_kw(t, Kw::Haste)), "tokens of a noncreature spell: no haste");
    // Thieving Skydiver kicked (X = 1): the {R}{R} pays {1} and the kicker.
    let mut g = arena(&["Thieving Skydiver"], &["Island"]);
    let ts = g.card(0, "Thieving Skydiver", Zone::Hand);
    cast_with_arena(&mut g, ts, |g| match g.kind() {
        Some(DecisionKind::ChooseCost) => Some(Choice::Number(1)),
        Some(DecisionKind::ChooseMode) => first_non_pass(g),
        _ => None,
    });
    assert_eq!(g.obj(ts).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.has_kw(ts, Kw::Haste), "the creature spell's cost, kicker included\n{}", log(&g));
    // Ragavan and a Test Dwarf: one {R} each.
    let mut g = arena(&["Ragavan, Nimble Pilferer", "Test Dwarf"], &["Mountain"]);
    let rg = g.card(0, "Ragavan, Nimble Pilferer", Zone::Hand);
    cast_with_arena(&mut g, rg, |g| (g.kind() == Some(DecisionKind::ChooseAltCost)).then_some(Choice::Number(0)));
    assert_eq!(g.player(0).pool.amt[3], 1, "one {{R}} left\n{}", log(&g));
    let dw = g.card(0, "Test Dwarf", Zone::Hand);
    g.choose(Choice::Cast(dw));
    resolve(&mut g);
    assert!(g.has_kw(rg, Kw::Haste) && g.has_kw(dw, Kw::Haste), "both creature spells\n{}", log(&g));
}

// ---- waterbend (Aang) ---------------------------------------------------------------------------------

#[test]
fn waterbend_taps_any_untapped_artifact_or_creature_but_not_one_tapped_for_mana_cr_601_2g() {
    // ruling: 5a445906 9257e4eb
    // Aang's waterbend {8}: four Islands, Sol Ring ({C}{C}), Aang himself
    // tapped for {1} and a Test Bear cast this turn (tapped for {1}:
    // summoning sickness doesn't matter): eight. Without the Bear, the Ring
    // can't both make {C}{C} and pay {1} by being tapped: seven, not
    // offered.
    let base = |bear: bool| {
        let mut g = scen()
            .battlefield(0, &["Aang, Swift Savior // Aang and La, Ocean's Fury", "Sol Ring", "Island", "Island", "Island", "Island"])
            .battlefield(0, if bear { &["Forest", "Forest"][..] } else { &[][..] })
            .hand(0, if bear { &["Test Bear"][..] } else { &[][..] })
            .at(Step::Main1, 0)
            .start();
        if bear {
            let b = g.card(0, "Test Bear", Zone::Hand);
            g.choose(Choice::Cast(b));
            resolve_with(&mut g, |g| match g.kind() {
                Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| matches!(c, Choice::Card(x) | Choice::Ability(x, _) if g.name(x) == "Forest")),
                _ => None,
            });
        }
        g
    };
    let g = base(false);
    let aang = bf(&g, 0, "Aang, Swift Savior // Aang and La, Ocean's Fury");
    let wb = (0..4u8).map(|i| ab(aang, i)).find(|&a| g.has_option(a));
    assert!(wb.is_none(), "four Islands, the Ring and Aang: seven: {}", opts(&g));
    let mut g = base(true);
    let bear = bf(&g, 0, "Test Bear");
    assert!(g.is_sick(bear));
    let aang = bf(&g, 0, "Aang, Swift Savior // Aang and La, Ocean's Fury");
    let wb = (0..4u8).map(|i| ab(aang, i)).find(|&a| g.has_option(a)).unwrap_or_else(|| panic!("waterbend offered: {}", opts(&g)));
    g.choose(wb);
    resolve(&mut g);
    assert!(g.obj(bear).tapped, "the Bear tapped for waterbend\n{}", log(&g));
    assert_eq!(g.name(bf(&g, 0, "Aang and La, Ocean's Fury")), "Aang and La, Ocean's Fury", "transformed");
}

#[test]
fn transmute_artifact_trading_down_pays_nothing_cr_701_19() {
    // ruling: 98bf27cb
    // Mind Stone sacrificed, Sol Ring found: onto the battlefield, no
    // payment asked.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Sol Ring", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Mind Stone", "Island", "Island"])
        .hand(0, &["Transmute Artifact"])
        .at(Step::Main1, 0)
        .start();
    let ta = g.card(0, "Transmute Artifact", Zone::Hand);
    let ms = bf(&g, 0, "Mind Stone");
    let ring = g.card(0, "Sol Ring", Zone::Library);
    let r = Choice::Def(g.obj(ring).def);
    g.choose(Choice::Cast(ta));
    let asked = std::cell::RefCell::new(Vec::new());
    resolve_with(&mut g, |g| {
        asked.borrow_mut().push(g.kind().unwrap());
        match g.kind() {
            Some(DecisionKind::PayMana) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|&c| !matches!(c, Choice::Card(x) | Choice::Ability(x, _) if x == ms)),
            _ if g.has_option(Choice::Card(ms)) => Some(Choice::Card(ms)),
            Some(DecisionKind::SearchLibrary) => Some(r),
            _ => None,
        }
    });
    assert_eq!(g.obj(ring).zone, Zone::Battlefield, "{}", log(&g));
    assert!(!asked.borrow().contains(&DecisionKind::YesNo), "nothing to pay: {:?}", asked.borrow());
}

#[test]
fn cascade_sees_a_split_cards_combined_mana_value_and_casts_one_half_cr_709_4_702_85a() {
    // ruling: 6187a75b
    // Bloodbraid Challenger (mana value 5) cascades past an Island into Life
    // // Death (mana value 3, {G} + {1}{B}): P0 casts Death, returning a
    // Test Bear; Life doesn't happen (P0's lands aren't creatures).
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island", "Life // Death", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Forest", "Forest"])
        .hand(0, &["Bloodbraid Challenger"])
        .graveyard(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bc = g.card(0, "Bloodbraid Challenger", Zone::Hand);
    let ld = g.card(0, "Life // Death", Zone::Library);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    g.choose(Choice::Cast(bc));
    let faces = std::cell::Cell::new(0usize);
    let tb = g.tgt(bear);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(Choice::Yes),
        Some(DecisionKind::ChooseFace) => {
            faces.set(g.pending_decision().unwrap().options.len());
            Some(Choice::Number(1))
        }
        Some(DecisionKind::Target) if g.has_option(tb) => Some(tb),
        _ => None,
    });
    assert_eq!(faces.get(), 2, "either half\n{}", log(&g));
    assert_eq!(g.obj(bear).zone, Zone::Battlefield, "Death\n{}", log(&g));
    assert_eq!(g.obj(ld).zone, Zone::Graveyard);
    let forest = bf(&g, 0, "Forest");
    assert!(!g.is_creature(forest), "not Life too");
}

#[test]
fn an_alternative_cost_with_retrace_or_with_elite_spellbinders_tax_cr_118_9_702_81a_601_2f() {
    // ruling: eb35fd18
    // Six's retrace adds a cost, it isn't an alternative one: Ragavan
    // retraced (a Forest discarded) may be cast for its dash cost, and has
    // haste. P1's Force of Will exiled by Elite Spellbinder: cast for its
    // pitch cost plus the {2}: not with one Island, yes with two.
    let mut g = scen()
        .battlefield(0, &["Six", "Mountain", "Mountain"])
        .graveyard(0, &["Ragavan, Nimble Pilferer"])
        .hand(0, &["Forest"])
        .at(Step::Main1, 0)
        .start();
    let r = g.card(0, "Ragavan, Nimble Pilferer", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(r)), "retrace: {}", opts(&g));
    let alts = std::cell::Cell::new(0usize);
    let asked = cast_recording(&mut g, Choice::Cast(r), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => {
            alts.set(g.pending_decision().unwrap().options.len());
            Some(Choice::Number(1))
        }
        _ => None,
    });
    assert!(asked.contains(&DecisionKind::ChooseAltCost), "{asked:?}\n{}", log(&g));
    assert_eq!(alts.get(), 2, "its mana cost or dash");
    assert_eq!(g.count_in(0, "Forest", Zone::Graveyard), 1, "retrace's land discarded\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.obj(r).zone, Zone::Battlefield, "{}", log(&g));
    assert!(g.has_kw(r, Kw::Haste), "dashed: haste\n{}", log(&g));
    assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 2, "{{1}}{{R}}");
    // Elite Spellbinder and Force of Will.
    for (islands, castable) in [(1usize, false), (2, true)] {
        let mut g = scen()
            .battlefield(0, &["Plains", "Plains", "Plains", "Mountain"])
            .hand(0, &["Elite Spellbinder", "Lightning Bolt"])
            .battlefield(1, &vec!["Island"; islands])
            .hand(1, &["Force of Will", "Brainstorm"])
            .at(Step::Main1, 0)
            .start();
        let fow = g.card(1, "Force of Will", Zone::Hand);
        let es = g.card(0, "Elite Spellbinder", Zone::Hand);
        g.choose(Choice::Cast(es));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(player(1)),
            _ if g.has_option(Choice::Card(fow)) => Some(Choice::Card(fow)),
            _ => None,
        });
        assert_eq!(g.obj(fow).zone, Zone::Exile, "{}", log(&g));
        let b = g.card(0, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(b));
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        g.choose(player(1));
        pay(&mut g);
        g.drive_until(|g| at_priority(g) && g.turn().priority == 0);
        g.pass();
        assert_eq!(g.has_option(Choice::Cast(fow)), castable, "{islands} Islands: {}", opts(&g));
        if castable {
            let tb = g.tgt(b);
            cast_recording(&mut g, Choice::Cast(fow), |g| match g.kind() {
                Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(1)),
                Some(DecisionKind::Target) => Some(tb),
                _ => None,
            });
            resolve(&mut g);
            assert_eq!(g.life(1), 19, "1 life, the Bolt countered\n{}", log(&g));
            assert_eq!(g.count_in(1, "Brainstorm", Zone::Exile), 1);
            assert_eq!(g.permanents_of(1).filter(|&c| g.obj(c).tapped).count(), 2, "the {{2}} paid");
        }
    }
}

#[test]
fn retrace_with_emerge_needs_the_land_card_to_discard_as_well_cr_702_81a_702_119a_601_2f() {
    // Six gives Crabomination in P0's graveyard retrace: cast it by
    // discarding a land card in addition to its other costs (CR 702.81a),
    // which may be its emerge cost (118.9; 702.119a: {5}{B}{B} and an
    // artifact sacrificed, less that artifact's mana value). The total cost
    // is both (601.2f-h). With only The One Ring in hand there is no land
    // card to discard: not castable (the match fuzz's match 1202 on main
    // 0054b32, ENGINE.md 40.9). With a Forest too: cast for emerge, the
    // Forest discarded, Golos (an artifact creature, mana value 5)
    // sacrificed, {B}{B} paid with two of the three Swamps.
    let setup = |hand: &[&str]| {
        scen()
            .battlefield(0, &["Six", "Golos, Tireless Pilgrim", "Swamp", "Swamp", "Swamp"])
            .graveyard(0, &["Crabomination"])
            .hand(0, hand)
            .library(1, &["Island", "Forest"])
            .hand(1, &["Forest"])
            .at(Step::Main1, 0)
            .start()
    };
    let g = setup(&["The One Ring"]);
    let crab = g.card(0, "Crabomination", Zone::Graveyard);
    assert!(!g.has_option(Choice::Cast(crab)), "no land card to discard: {}", opts(&g));
    let mut g = setup(&["The One Ring", "Forest"]);
    let crab = g.card(0, "Crabomination", Zone::Graveyard);
    let golos = bf(&g, 0, "Golos, Tireless Pilgrim");
    let forest = g.card(0, "Forest", Zone::Hand);
    assert!(g.has_option(Choice::Cast(crab)), "retrace with emerge: {}", opts(&g));
    let alts = std::cell::Cell::new(0usize);
    let asked = cast_recording(&mut g, Choice::Cast(crab), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => {
            alts.set(g.pending_decision().unwrap().options.len());
            Some(Choice::Number(1))
        }
        Some(DecisionKind::Sacrifice) => Some(Choice::Card(golos)),
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    assert!(asked.contains(&DecisionKind::Sacrifice) && asked.contains(&DecisionKind::DiscardCost), "{asked:?}\n{}", log(&g));
    assert_eq!(alts.get(), 1, "emerge only: its mana cost {{4}}{{B}}{{B}} can't be paid");
    assert!(log(&g).contains("Retrace"), "cast with retrace\n{}", log(&g));
    assert_eq!(g.obj(forest).zone, Zone::Graveyard, "the land card discarded\n{}", log(&g));
    assert_eq!(g.obj(golos).zone, Zone::Graveyard, "Golos sacrificed\n{}", log(&g));
    assert_eq!(g.count_in(0, "The One Ring", Zone::Hand), 1);
    assert_eq!(g.permanents_of(0).filter(|&c| g.obj(c).tapped).count(), 2, "{{5}}{{B}}{{B}} less 5: {{B}}{{B}}\n{}", log(&g));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        Some(DecisionKind::ChooseExiled) => Some(Choice::Pass),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Crabomination", Zone::Battlefield), 1, "{}", log(&g));
}
