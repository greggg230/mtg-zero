//! Workstream A of the faithfulness audit (ENGINE.md 100.2,
//! docs/faithfulness_audit.md 5.1-5.2): every legal option of a choice is
//! reachable inside the fixed action ranges, by paging (`Game::ask_paged`)
//! or by a big number's digits (`Game::ask_number`), with nothing truncated
//! or clamped. Each test names the CR rule whose choice it checks.

mod common;

use engine::db::CardDb;
use engine::decision::act;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step};
use std::sync::OnceLock;

/// The cube's cards and the Edge of Eternities' (as `rules_faithful_c.rs`).
fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        for c in engine::db::load_eoe_document().cards {
            if !cards.iter().any(|x| x.name == c.name) {
                cards.push(c);
            }
        }
        Box::leak(Box::new(CardDb::from_cards(&cards)))
    })
}

fn scen() -> Scenario {
    Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

fn actions(g: &Game) -> Vec<u16> {
    g.pending_decision().unwrap().options.iter().map(|o| o.action).collect()
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// The pending decision's "next page" option (ENGINE.md 100.1).
fn next_page(g: &Game) -> Choice {
    options(g).into_iter().find(|c| matches!(c, Choice::Page(_))).unwrap_or_else(|| panic!("no next page: {:?}", g.options_desc()))
}

/// A determinized world of the decider asks the same decision (ENGINE.md
/// 10, 100.1: the page or the digits live in the asking task).
fn same_after_determinize(g: &Game) {
    let p = g.pending_decision().unwrap().player;
    let mut d = g.clone();
    d.determinize(p, &mut engine::rng::Rng::new(11));
    assert_eq!(d.pending_decision(), g.pending_decision(), "determinize changed the decision");
}

// ---- 31.7.8: every distinct waiting trigger may be put on the stack first (CR 603.3b) ----

/// Eighteen Ouroboroids ("at the beginning of combat on
/// your turn, put X +1/+1 counters on each creature you control, where X is
/// this creature's power"): eighteen triggers that read their own source,
/// none interchangeable (CR 603.3b, 113.7), so their controller may put any
/// of them on the stack first. The order decision offers the first 15 at
/// `TRIGGER` + 0..=14 and "next page" at `TRIGGER` + 15, then the other
/// three at `TRIGGER` + 0..=2 (ENGINE.md 100.2; before it, the 17th and 18th
/// could never go first, docs/shortcuts.md 31.7.8).
#[test]
fn cr_603_3b_any_of_eighteen_distinct_triggers_goes_on_the_stack_first() {
    let mut g = scen().battlefield(0, &["Ouroboroid"; 18]).at(Step::Main1, 0).start();
    let os: Vec<CardIdx> = g.permanents_of(0).filter(|&c| g.name(c) == "Ouroboroid").collect();
    assert_eq!(os.len(), 18);
    g.pass();
    g.drive_until(|g| g.kind() == Some(DecisionKind::OrderTriggers));
    assert_eq!(actions(&g), (act::TRIGGER..act::TRIGGER + 16).collect::<Vec<_>>(), "{:?}", g.options_desc());
    assert!(matches!(options(&g)[15], Choice::Page(_)));
    g.choose(next_page(&g));
    same_after_determinize(&g);
    assert_eq!(g.kind(), Some(DecisionKind::OrderTriggers));
    assert_eq!(actions(&g), vec![act::TRIGGER, act::TRIGGER + 1, act::TRIGGER + 2]);
    assert_eq!(options(&g), vec![Choice::Trigger(15), Choice::Trigger(16), Choice::Trigger(17)]);
    let last = g.waiting_triggers().iter().filter(|t| t.controller == 0).nth(17).unwrap().source;
    g.choose(Choice::Trigger(17));
    g.drive_until(|g| g.kind() == Some(DecisionKind::OrderTriggers));
    match g.stack()[0].kind {
        engine::game::StackKind::Ability { source, .. } => assert_eq!(source, last, "the 18th trigger went first\n{}", log(&g)),
        k => panic!("{k:?}"),
    }
    // The next order decision starts at the first page again: 17 distinct
    // triggers, the first page's 15 and "next page".
    assert_eq!(actions(&g), (act::TRIGGER..act::TRIGGER + 16).collect::<Vec<_>>());
}

// ---- 31.7.5, N14: any card name (CR 201.3) ------------------------------------------------

/// Anointed Peacekeeper ("as this enters, look at an opponent's hand, then
/// choose any card name ... activated abilities of sources with the chosen
/// name cost {2} more"): the names are those of every card in the game, the
/// chooser's own too (the tax is on every source's abilities), and of the
/// table's tokens whose names are card names (ENGINE.md 127; 72 cards of one
/// face here): the first page offers 63 at `LIBRARY` + 0..=62, "next page"
/// at `LIBRARY` + 63 and `PASS` (a name no spell or source of the game can
/// have); the second the rest at `LIBRARY` + 0.. (ENGINE.md 100.2; before it
/// the opponent's names only, cut at 64: docs/shortcuts.md 31.7.5, the
/// audit's N14). One of the chooser's own cards' names is chosen.
#[test]
fn cr_201_3_peacekeeper_may_name_any_card_in_the_game_even_its_controllers() {
    use engine::ids::DefId;
    use engine::sneak_show::face_name;
    let one_face = |d: DefId| {
        let dd = db().def(d);
        dd.halves.is_none() && dd.back.is_none() && dd.adventure.is_none() && dd.prepare.is_none()
    };
    let mut names: Vec<&str> = db()
        .supported()
        .filter(|n| {
            !["Anointed Peacekeeper", "Plains", "Island"].contains(n)
                && db().id(n).is_some_and(|d| !db().def(d).is(engine::db::types::LAND) && one_face(d))
        })
        .collect();
    names.sort_unstable();
    names.truncate(70);
    assert_eq!(names.len(), 70);
    let mut g = Scenario::new(db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &names[30..])
        .library(1, &names[..30])
        .hand(0, &["Anointed Peacekeeper"])
        .battlefield(0, &["Plains"; 3])
        .at(Step::Main1, 0)
        .start();
    let pk = g.card(0, "Anointed Peacekeeper", engine::Zone::Hand);
    g.choose(Choice::Cast(pk));
    for _ in 0..20 {
        if at_priority(&g) && g.stack().is_empty() {
            break;
        }
        if g.kind() == Some(DecisionKind::NameCard) {
            break;
        }
        let c = if at_priority(&g) { Choice::Pass } else { options(&g)[0] };
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::NameCard), "{}", log(&g));
    let mut want: Vec<u16> = vec![act::PASS];
    want.extend(act::LIBRARY..act::LIBRARY + 64);
    assert_eq!(actions(&g), want);
    let mut cards: Vec<DefId> = names.iter().map(|n| db().id(n).unwrap()).collect();
    cards.push(db().id("Anointed Peacekeeper").unwrap());
    cards.push(db().id("Plains").unwrap());
    cards.sort_unstable();
    cards.dedup();
    assert_eq!(cards.len(), 72);
    // The table's tokens with a card's name (a face of a card of the table,
    // or Faerie Dragon: CR 201.3), one definition a name, the smallest.
    let n = db().num_cards();
    let card_faces: Vec<&str> = (0..db().len() as DefId)
        .filter(|&d| {
            let dd = db().def(d);
            !dd.token && ((d as usize) < n || dd.front.is_some_and(|f| (f as usize) < n))
        })
        .map(|d| face_name(db().def(d)))
        .collect();
    let mut defs = cards.clone();
    defs.extend((n..db().len()).map(|t| t as DefId).filter(|&t| {
        let d = db().def(t);
        d.token && !d.emblem && !d.rules && (card_faces.contains(&face_name(d)) || d.name == "Faerie Dragon")
    }));
    defs.sort_unstable_by(|&a, &b| face_name(db().def(a)).cmp(face_name(db().def(b))).then(a.cmp(&b)));
    defs.dedup_by(|a, b| face_name(db().def(*a)) == face_name(db().def(*b)));
    defs.sort_unstable();
    assert!(defs.len() > 64 && defs.len() <= 63 + 64, "two pages: {}", defs.len());
    assert_eq!(options(&g)[1..64], defs[..63].iter().map(|&d| Choice::Def(d)).collect::<Vec<_>>()[..]);
    g.choose(next_page(&g));
    same_after_determinize(&g);
    assert_eq!(options(&g), defs[63..].iter().map(|&d| Choice::Def(d)).collect::<Vec<_>>());
    assert_eq!(actions(&g), (act::LIBRARY..act::LIBRARY + (defs.len() - 63) as u16).collect::<Vec<_>>());
    let own = *defs[63..].iter().find(|&&d| names[30..].iter().any(|n| db().id(n) == Some(d))).expect("one of P0's names on page 2");
    g.choose(Choice::Def(own));
    assert!(g.log_lines().iter().any(|l| l.contains(&format!("chosen name: {}", db().def(own).name))), "{}", log(&g));
}

// ---- 13.9, 23.11.3, N5: any number (CR 510.1c-d, 601.2b, 107.3) ---------------------------

/// Answer a big number: `NUMBER` + v below `lo` + 31, else "31 or more" and
/// the rest's base-32 digits (ENGINE.md 100.1), checking each decision.
fn choose_number(g: &mut Game, lo: u64, hi: u64, v: u64) {
    if hi - lo <= 31 || v - lo < 31 {
        g.choose(Choice::Number((v - if hi <= 31 { 0 } else { lo }) as u8));
        return;
    }
    assert_eq!(actions(g), (act::NUMBER..act::NUMBER + 32).collect::<Vec<_>>(), "the first decision: 0..=30 and 31 or more");
    g.choose(Choice::Number(31));
    let rest = v - lo - 31;
    let top = hi - lo - 31;
    let mut n = 1;
    while top >> (5 * n) > 0 {
        n += 1;
    }
    for i in (0..n).rev() {
        same_after_determinize(g);
        let d = (rest >> (5 * i)) & 31;
        g.choose(Choice::Number(d as u8));
    }
}

/// A 40/40 attacker blocked by two 51/51s (CR 510.1c-d): the attacking
/// player may assign the first blocker any amount from 0 to 40, here 37
/// ("31 or more", then the digit 6), and the last blocker the other 3.
/// Before ENGINE.md 100.2 the amounts offered were 0..=29, lethal and all
/// (docs/shortcuts.md 13.9, 23.11.3).
#[test]
fn cr_510_1c_any_division_of_32_or_more_combat_damage() {
    use engine::db::Counter;
    let mut g = scen()
        .battlefield(0, &["Llanowar Elves"])
        .counters(0, "Llanowar Elves", Counter::P1P1, 39)
        .battlefield(1, &["Ouroboroid", "Tishana's Tidebinder"])
        .counters(1, "Ouroboroid", Counter::P1P1, 50)
        .counters(1, "Tishana's Tidebinder", Counter::P1P1, 50)
        .at(Step::Main1, 0)
        .start();
    let elves = g.card(0, "Llanowar Elves", engine::Zone::Battlefield);
    assert_eq!(g.power(elves), 40);
    for _ in 0..200 {
        match g.kind() {
            Some(DecisionKind::AssignDamage) => break,
            Some(DecisionKind::DeclareAttackers) => {
                let c = if g.has_option(Choice::Card(elves)) { Choice::Card(elves) } else { Choice::Pass };
                g.choose(c);
            }
            Some(DecisionKind::DeclareBlockers) => {
                g.choose(Choice::Card(elves));
            }
            Some(DecisionKind::Priority) => {
                g.pass();
            }
            _ => {
                let c = options(&g)[0];
                g.choose(c);
            }
        }
    }
    assert_eq!(g.kind(), Some(DecisionKind::AssignDamage), "{}", log(&g));
    let blockers = g.blockers_of(elves);
    assert_eq!(blockers.len(), 2);
    choose_number(&mut g, 0, 40, 37);
    assert_eq!(g.kind(), Some(DecisionKind::AssignDamage));
    assert_eq!(actions(&g), vec![act::NUMBER + 3], "the last blocker gets the rest");
    g.choose(Choice::Number(3));
    g.drive_until(at_priority);
    assert_eq!((g.obj(blockers[0]).damage, g.obj(blockers[1]).damage), (37, 3), "{}", log(&g));
}

/// Walking Ballista ({X}{X}, "enters with X +1/+1 counters") with 70
/// colorless mana: X may be anything to 35 (CR 107.3, 601.2b), here 33.
/// Before ENGINE.md 100.2, at most 31 (the audit's N5).
#[test]
fn cr_601_2b_an_x_of_33() {
    let mut g = scen().hand(0, &["Walking Ballista"]).pool(0, [0, 0, 0, 0, 0, 70], 0).at(Step::Main1, 0).start();
    let wb = g.card(0, "Walking Ballista", engine::Zone::Hand);
    g.choose(Choice::Cast(wb));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    choose_number(&mut g, 0, 35, 33);
    for _ in 0..50 {
        if at_priority(&g) && g.stack().is_empty() {
            break;
        }
        if at_priority(&g) {
            g.pass();
        } else {
            let c = options(&g)[0];
            g.choose(c);
        }
    }
    assert_eq!(g.obj(wb).zone, engine::Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.counters_on(wb, engine::db::Counter::P1P1), 33, "{}", log(&g));
}

/// Toxic Deluge ("as an additional cost, pay X life; all creatures get
/// -X/-X") at 50 life: X may be anything to 50 (CR 119.4, 601.2b), here 40.
/// Before ENGINE.md 100.2, at most 31 (N5).
#[test]
fn cr_119_4_pay_40_life() {
    let mut g = scen().hand(0, &["Toxic Deluge"]).pool(0, [0, 0, 3, 0, 0, 0], 0).life(0, 50).at(Step::Main1, 0).start();
    let td = g.card(0, "Toxic Deluge", engine::Zone::Hand);
    g.choose(Choice::Cast(td));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    choose_number(&mut g, 0, 50, 40);
    g.drive_until(|g| at_priority(g));
    assert_eq!(g.player(0).life, 10, "{}", log(&g));
}

// ---- N12: any counter kind (CR 603.12, 122.1) ---------------------------------------------

/// Leatherhead, Swamp Stalker ("whenever this deals combat damage to a
/// player, you may remove a counter from it; when you do, destroy target
/// artifact or enchantment") with eighteen kinds of counters: each kind may
/// be the one removed (CR 122.1), `NUMBER` + 0..=17 (before ENGINE.md 100.2
/// the first 16 only: the audit's N12; past 32 kinds the choice pages).
#[test]
fn cr_122_1_any_of_eighteen_counter_kinds_may_be_removed() {
    use engine::db::{Counter, Kw};
    let kinds = [
        Counter::P1P1,
        Counter::Kw(Kw::Flying),
        Counter::Kw(Kw::Reach),
        Counter::Kw(Kw::FirstStrike),
        Counter::Kw(Kw::Deathtouch),
        Counter::Kw(Kw::Lifelink),
        Counter::Kw(Kw::Vigilance),
        Counter::Kw(Kw::Haste),
        Counter::Kw(Kw::Menace),
        Counter::Kw(Kw::Indestructible),
        Counter::Kw(Kw::Shadow),
        Counter::Kw(Kw::Trample),
        Counter::Kw(Kw::Flash),
        Counter::Kw(Kw::Shroud),
        Counter::Kw(Kw::Hexproof),
        Counter::Lore,
        Counter::Loyalty,
        Counter::Stun,
    ];
    let mut s = scen().battlefield(0, &["Leatherhead, Swamp Stalker"]).battlefield(1, &["Sol Ring"]).at(Step::Main1, 0);
    for k in kinds {
        s = s.counters(0, "Leatherhead, Swamp Stalker", k, 1);
    }
    let mut g = s.start();
    let lh = g.card(0, "Leatherhead, Swamp Stalker", engine::Zone::Battlefield);
    let all = g.counters_of(lh);
    assert_eq!(all.len(), 18, "{all:?}");
    for _ in 0..200 {
        match g.kind() {
            Some(DecisionKind::ChooseMode) => break,
            Some(DecisionKind::DeclareAttackers) => {
                let c = if g.has_option(Choice::Card(lh)) { Choice::Card(lh) } else { Choice::Pass };
                g.choose(c);
            }
            Some(DecisionKind::Priority) => {
                g.pass();
            }
            _ => {
                let c = options(&g)[0];
                g.choose(c);
            }
        }
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMode), "{}", log(&g));
    let mut want = vec![act::PASS];
    want.extend(act::NUMBER..act::NUMBER + 18);
    assert_eq!(actions(&g), want);
    let last = all[17].0;
    g.choose(Choice::Number(17));
    assert_eq!(g.counters_on(lh, last), 0, "the 18th kind is removed\n{}", log(&g));
}

/// The identity a block slot at `o` holds, if present (its card code's three
/// one-hot digits; present: the status's first float).
fn slot_identity(x: &[f32], o: usize) -> Option<usize> {
    use engine::az::encode::CODE;
    let digit = |a: usize, n: usize| x[o + a..o + a + n].iter().position(|&v| v == 1.0);
    (x[o + CODE] == 1.0).then(|| digit(0, 16).unwrap() * 64 + digit(16, 8).unwrap() * 8 + digit(24, 8).unwrap())
}

/// On a page of the trigger order, the triggers block shows the page's
/// triggers at their slots (ENGINE.md 100.5.1: `TRIGGER` + j is the trigger
/// at place 15 + j on the second page): eighteen Ouroboroid triggers, the
/// second page holds three, in slots 0..2, and slot 3 is empty (the first
/// page: four).
#[test]
fn cr_603_3b_a_page_of_triggers_is_the_trigger_blocks() {
    use engine::az::encode::{O_TRIG, TRIGS, TRIG_W};
    let mut g = scen().battlefield(0, &["Ouroboroid"; 18]).at(Step::Main1, 0).start();
    g.pass();
    g.drive_until(|g| g.kind() == Some(DecisionKind::OrderTriggers));
    let present = |g: &Game| {
        let mut x = vec![0.0f32; engine::az::NUM_FEATURES];
        g.encode_features(0, &mut x);
        (0..TRIGS).map(|k| slot_identity(&x, O_TRIG + k * TRIG_W).is_some()).collect::<Vec<bool>>()
    };
    assert_eq!(present(&g), [true; 4]);
    g.choose(next_page(&g));
    assert_eq!(actions(&g), vec![act::TRIGGER, act::TRIGGER + 1, act::TRIGGER + 2]);
    assert_eq!(present(&g), [true, true, true, false], "places 15..17");
}
