//! Workstream B of the faithfulness audit (docs/faithfulness_audit.md 5.3,
//! 5.4; ENGINE.md 100.4): mana and payment, exact. Each test names the CR
//! rule it checks and failed before its fix.

mod common;
mod eoe_w2;

use engine::db::{AbilitySlot, CardDb, MANA_UNCOUNTERABLE, SPEND_ARTIFACT, SPEND_LEGENDARY};
use engine::mana::{self, ManaCost};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

/// The cube with the shared test cards.
fn scen() -> Scenario {
    Scenario::new(common::db())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 12])
        .library(1, &["Island"; 12])
}

/// Answer decisions (`answer`, else the first option) until priority.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..400 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress\n{}", log(g));
}

/// Run the stack down, answering with the first option.
fn settle(g: &mut Game) {
    for _ in 0..300 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
        } else {
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
    }
    panic!("settle: no progress\n{}", log(g));
}

/// Every option of a (possibly paged) decision, applied to a copy, then
/// `finish`: "next page" options are followed, so each item once.
fn each_item(g: &Game) -> Vec<Game> {
    let mut out = Vec::new();
    for c in options(g) {
        let mut h = g.clone();
        h.choose(c);
        if matches!(c, Choice::Page(_)) {
            out.extend(each_item(&h));
        } else {
            finish(&mut h, |_| None);
            out.push(h);
        }
    }
    out
}

// ---- 100.4.1: the feasibility search -----------------------------------------

/// Gene Pollinator ("{T}, Tap an untapped permanent you control: Add one
/// mana of any color") with many lands: every land may be tapped for its own
/// ability or for a Pollinator's (CR 601.2g, 605.3a), and the payment's
/// feasibility is decided exactly, whatever the number of lands (EoE fuzz
/// seed 10023: two Pollinators and 13 lands made each check a search of 3^13
/// leaves, made thousands of times per decision; with 20 lands one check
/// passed the search's node limit, a loud failure; ENGINE.md 100.4).
#[test]
fn cr_601_2g_two_gene_pollinators_and_twenty_lands_are_decided_exactly() {
    let lands = ["Island", "Swamp", "Mountain", "Forest"].repeat(5);
    let mut bfs: Vec<&str> = vec!["Gene Pollinator", "Gene Pollinator"];
    bfs.extend(lands.iter().copied());
    let g = eoe_w2::scen().battlefield(0, &bfs).at(Step::Main1, 0).start();
    // Each Pollinator taps a land for {W}: two {W}, eighteen lands left.
    assert!(g.can_pay(0, &cost("{W}{W}{18}")));
    // One more is too much: a third {W} needs a third activation, and a
    // Pollinator tapping the other makes one {W} where two lands made two.
    assert!(!g.can_pay(0, &cost("{W}{W}{19}")));
    assert!(!g.can_pay(0, &cost("{W}{W}{W}")));
    // Twenty-one: one Pollinator taps the other, every land for itself.
    assert!(g.can_pay(0, &cost("{W}{20}")));
    assert!(!g.can_pay(0, &cost("{22}")));
}

// ---- 100.4.2: the pool's mana: restrictions, riders, every remainder ---------

/// CR 106.6: a spending restriction goes with the mana. Mishra's Workshop's
/// {C}{C}{C} floated in response to Wasteland (ENGINE.md 86) is still
/// "only to cast artifact spells" in the pool (docs/shortcuts.md N19: the
/// cube's table dropped the restriction).
#[test]
fn cr_106_6_floated_workshop_mana_keeps_its_restriction() {
    let mut g = Scenario::new(CardDb::constructed())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(1, &["Wasteland"])
        .battlefield(0, &["Mishra's Workshop"])
        .at(Step::Main1, 1)
        .start();
    let waste = bf(&g, 1, "Wasteland");
    let ws = bf(&g, 0, "Mishra's Workshop");
    g.choose(act(waste, 1));
    g.choose(g.tgt(ws));
    g.drive_until(at_priority);
    g.pass();
    assert!(at_priority(&g) && g.pending_decision().unwrap().player == 0);
    assert!(g.has_option(act(ws, 0)), "{:?}", g.options_desc());
    g.choose(act(ws, 0));
    finish(&mut g, |_| None);
    assert_eq!(g.player(0).pool.amt[mana::C], 3, "{}", log(&g));
    assert_eq!(g.restricted_mana(0), vec![(mana::C as u8, SPEND_ARTIFACT, 3)], "artifact spells only");
    assert!(!g.can_pay(0, &cost("{1}")), "not for anything else");
}

/// CR 106.6: a rider goes with the mana as well. Delighted Halfling's "Spend
/// this mana only to cast a legendary spell, and that spell can't be
/// countered": a legendary spell paid with such mana from the pool can't be
/// countered (docs/shortcuts.md N20: the pool had no such tag).
#[test]
fn cr_106_6_the_halfling_rider_stays_with_mana_in_the_pool() {
    let mut g = scen()
        .battlefield(0, &["Forest"])
        .pool_restricted(0, mana::W, SPEND_LEGENDARY | MANA_UNCOUNTERABLE, 1)
        .battlefield(1, &["Island", "Island"])
        .hand(0, &["Test Legend"])
        .hand(1, &["Counterspell"])
        .at(Step::Main1, 0)
        .start();
    let leg = g.card(0, "Test Legend", Zone::Hand);
    g.choose(Choice::Cast(leg));
    finish(&mut g, |_| None);
    assert_eq!(g.player(0).pool.amt, [0; 6], "the pool's {{W}} paid: {}", log(&g));
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    assert!(g.has_option(Choice::Cast(cs)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(cs));
    finish(&mut g, |_| None);
    settle(&mut g);
    assert_eq!(g.obj(leg).zone, Zone::Battlefield, "it can't be countered: {}", log(&g));
    assert_eq!(g.obj(cs).zone, Zone::Graveyard);
}

/// CR 106.4, 601.2h: which pool mana pays is the player's choice, also
/// between Mishra's Workshop's artifact-only {C} and another {C} (13.1k:
/// the restricted {C} was always spent first, "never worse"; the pools left
/// differ, and nothing turns plain {C} into the restricted kind).
#[test]
fn cr_601_2h_restricted_or_plain_colorless_for_an_artifact() {
    let mut g = scen().pool(0, [0, 0, 0, 0, 0, 1], 0).pool_restricted(0, mana::C, SPEND_ARTIFACT, 1).hand(0, &["Sol Ring"]).at(Step::Main1, 0).start();
    let sr = g.card(0, "Sol Ring", Zone::Hand);
    g.choose(Choice::Cast(sr));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<u8> = each_item(&g).iter().map(|h| h.player(0).pool.art).collect();
    left.sort();
    assert_eq!(left, vec![0, 1], "the plain {{C}} left, or the restricted one");
}

/// CR 106.4, 601.2h (Arena of Glory's haste mana): on a noncreature spell
/// too, the haste {R} or another {R} may pay (13.1k: the haste {R} was always
/// kept).
#[test]
fn cr_601_2h_haste_red_may_pay_a_noncreature_spell() {
    let mut g = scen().pool(0, [0, 0, 0, 2, 0, 0], 1).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(engine::Target::Player(1)));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<u8> = each_item(&g).iter().map(|h| h.player(0).pool.haste).collect();
    left.sort();
    assert_eq!(left, vec![0, 1]);
}

/// CR 106.4, 601.2h: every pool a payment can leave is offered, however many
/// (docs/shortcuts.md N11: at most 32, after a ranking). Three of each type
/// in the pool and a {6} spell: 336 pools left, paged (A0's helper).
#[test]
fn cr_601_2h_every_pool_left_is_offered() {
    let mut g = scen().pool(0, [3, 3, 3, 3, 3, 3], 0).hand(0, &["Coveted Jewel"]).at(Step::Main1, 0).start();
    let w = g.card(0, "Coveted Jewel", Zone::Hand);
    g.choose(Choice::Cast(w));
    assert_eq!(g.kind(), Some(DecisionKind::PayPool), "{}", log(&g));
    let mut left: Vec<[u8; 6]> = each_item(&g).iter().map(|h| h.player(0).pool.amt).collect();
    let n = left.len();
    left.sort();
    left.dedup();
    assert_eq!((n, left.len()), (336, 336), "each pool once");
    assert!(left.iter().all(|a| a.iter().map(|&x| x as u32).sum::<u32>() == 12 && a.iter().all(|&x| x <= 3)));
}
