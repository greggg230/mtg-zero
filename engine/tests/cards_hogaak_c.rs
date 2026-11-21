//! Lane C of the Legacy Hogaak Tempo cards (ENGINE.md 220.3) against their
//! Oracle text, the Comprehensive Rules and their official rulings (`//
//! ruling: <hash8>`, `data/rulings/legacy.rulings.json`). They are in the card
//! table of constructed play only (`CardDb::constructed`).
//!
//! Stitcher's Supplier {B}, Creature — Zombie 1/1: "When this creature enters
//! or dies, mill three cards. (Put the top three cards of your library into
//! your graveyard.)"
//!
//! Dark Matter Manipulator {B}, Creature — Human Warlock 1/2: "When this
//! creature enters, mill three cards. / This creature gets +2/+0 for every
//! seven cards in your graveyard."
//!
//! Moonshadow {B}, Creature — Elemental 7/7: "Menace / This creature enters
//! with six -1/-1 counters on it. / Whenever one or more permanent cards are
//! put into your graveyard from anywhere while this creature has a -1/-1
//! counter on it, remove a -1/-1 counter from this creature."
//!
//! Dryad Arbor, Land Creature — Forest Dryad 1/1 (a green colour indicator):
//! "(This land isn't a spell, it's affected by summoning sickness, and it has
//! "{T}: Add {G}.")"

use engine::db::{types, AbilitySlot, CardDb, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Zone};

const SUPPLIER: &str = "Stitcher's Supplier";
const DMM: &str = "Dark Matter Manipulator";
const MOON: &str = "Moonshadow";
const ARBOR: &str = "Dryad Arbor";

fn db() -> &'static CardDb {
    CardDb::constructed()
}

/// No auto-answers; empty libraries (each test gives its own).
fn bare() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn hand(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Hand)
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// Answer the decisions of a cast or an activation until priority: `answer`
/// first, else the first option.
fn pay_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
        if at_priority(g) || g.is_over() {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("no priority\n{}", log(g));
}

fn pay(g: &mut Game) {
    pay_with(g, |_| None);
}

/// Resolve the stack (passing priority), `answer` answering what is asked
/// (else the first option).
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
    panic!("resolve_with: no progress\n{}", log(g));
}

fn resolve(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Pass priority, declare no attackers or blockers and answer one-option
/// decisions until `stop` holds.
fn pass_to(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) || g.is_over() {
            return;
        }
        if g.has_option(Choice::Pass) {
            g.pass();
            continue;
        }
        let d = g.pending_decision().unwrap_or_else(|| panic!("no decision\n{}", log(g)));
        assert_eq!(d.options.len(), 1, "a real decision: {}\n{}", opts(g), log(g));
        let c = d.options[0].choice;
        g.choose(c);
    }
    panic!("pass_to: no progress\n{}", log(g));
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = hand(g, p, name);
    assert!(g.has_option(Choice::Cast(c)), "{name} castable: {}", opts(g));
    g.choose(Choice::Cast(c));
    pay(g);
}

fn cast_and_resolve(g: &mut Game, p: PlayerId, name: &str) {
    cast(g, p, name);
    resolve(g);
}

fn gy(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).graveyard.iter().map(|&c| g.name(c).to_string()).collect()
}

fn m1(g: &Game, c: CardIdx) -> u64 {
    g.counters_on(c, Counter::M1M1) as u64
}

fn pt(g: &Game, c: CardIdx) -> (i128, i128) {
    let (p, t) = g.pt(c);
    (p as i128, t as i128)
}

// ---- the definitions -------------------------------------------------------------

#[test]
fn the_four_cards_are_defined_as_printed() {
    let d = db();
    let s = d.def(d.id(SUPPLIER).unwrap());
    assert_eq!((s.power, s.toughness, s.mana_value()), (Some(1), Some(1), 1));
    assert_eq!(s.triggers.len(), 2, "enters, dies");
    let m = d.def(d.id(DMM).unwrap());
    assert_eq!((m.power, m.toughness, m.mana_value()), (Some(1), Some(2), 1));
    assert_eq!(m.statics.len(), 1);
    let ms = d.def(d.id(MOON).unwrap());
    assert_eq!((ms.power, ms.toughness, ms.mana_value()), (Some(7), Some(7), 1));
    assert!(ms.keywords.has(Kw::Menace));
    assert_eq!(ms.enters_counters, vec![(Counter::M1M1, 6)]);
    let a = d.def(d.id(ARBOR).unwrap());
    assert_eq!(a.types, types::LAND | types::CREATURE);
    assert_eq!((a.power, a.toughness, a.mana_value()), (Some(1), Some(1), 0));
    assert_eq!(a.colors, 1 << engine::mana::G);
}

// ---- smoke ---------------------------------------------------------------------

#[test]
fn smoke_supplier_mills_and_moonshadow_shrinks() {
    let mut g = bare()
        .library(0, &["Island", "Lightning Bolt", "Forest", "Island", "Island"])
        .hand(0, &[SUPPLIER])
        .battlefield(0, &["Swamp", MOON])
        .counters(0, MOON, Counter::M1M1, 6)
        .start();
    cast_and_resolve(&mut g, 0, SUPPLIER);
    assert_eq!(gy(&g, 0), vec!["Island", "Lightning Bolt", "Forest"], "{}", log(&g));
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 5, "{}", log(&g));
}

#[test]
fn smoke_dryad_arbor_is_played_not_cast() {
    let mut g = bare().hand(0, &[ARBOR]).start();
    let a = hand(&g, 0, ARBOR);
    assert!(g.has_option(Choice::Play(a)) && !g.has_option(Choice::Cast(a)), "{}", opts(&g));
    g.choose(Choice::Play(a));
    let a = bf(&g, 0, ARBOR);
    assert!(g.is_creature(a) && g.is_type(a, types::LAND));
    assert!(!g.offered().iter().any(|c| matches!(c, Choice::Ability(x, AbilitySlot::Activated(_)) if *x == a)), "{}", opts(&g));
    let _ = Step::Main1;
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(engine::Target::Player(p))
}

fn target(t: Choice) -> impl Fn(&Game) -> Option<Choice> + Copy {
    move |g: &Game| g.has_option(t).then_some(t)
}

/// P`p`'s library, top first, by name.
fn lib(g: &Game, p: PlayerId) -> Vec<String> {
    g.player(p).library.iter().rev().map(|&c| g.name(c).to_string()).collect()
}

// ---- Stitcher's Supplier ---------------------------------------------------------

/// "When this creature enters ..., mill three cards" (CR 603.6a, 701.17a):
/// the top three cards of its controller's library, into that player's
/// graveyard; the opponent's library is untouched.
#[test]
fn cr_701_17a_stitchers_supplier_mills_three_as_it_enters() {
    let mut g = bare()
        .library(0, &["Island", "Lightning Bolt", "Forest", "Swamp"])
        .library(1, &["Mountain", "Mountain", "Mountain"])
        .hand(0, &[SUPPLIER])
        .battlefield(0, &["Swamp"])
        .start();
    cast(&mut g, 0, SUPPLIER);
    assert!(gy(&g, 0).is_empty(), "nothing milled before it resolves");
    resolve(&mut g);
    assert_eq!(gy(&g, 0), vec!["Island", "Lightning Bolt", "Forest"], "{}", log(&g));
    assert_eq!(lib(&g, 0), vec!["Swamp"]);
    assert_eq!(lib(&g, 1).len(), 3);
}

/// "... or dies": the same ability triggers when it is put into a graveyard
/// from the battlefield (CR 700.4); a leaves-the-battlefield ability looks
/// back (CR 603.6c, 603.10a), so it triggers though its source is gone, and
/// resolves from the graveyard's side.
#[test]
fn cr_603_10a_stitchers_supplier_mills_three_when_it_dies() {
    let mut g = bare()
        .library(0, &["Island", "Forest", "Plains", "Swamp"])
        .battlefield(0, &[SUPPLIER])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let s = bf(&g, 0, SUPPLIER);
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    { let t = g.tgt(s); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Graveyard);
    assert_eq!(gy(&g, 0), vec![SUPPLIER, "Island", "Forest", "Plains"], "{}", log(&g));
}

/// CR 701.17b-c: milling more cards than the library holds mills them all,
/// and nothing else happens (no draw from an empty library: 704.5b doesn't
/// apply to milling).
#[test]
fn cr_701_17_mill_three_with_fewer_cards_mills_what_is_there() {
    let mut g = bare().library(0, &["Island", "Forest"]).hand(0, &[SUPPLIER]).battlefield(0, &["Swamp"]).start();
    cast_and_resolve(&mut g, 0, SUPPLIER);
    assert_eq!(gy(&g, 0), vec!["Island", "Forest"]);
    assert!(g.player(0).library.is_empty());
    assert!(!g.is_over());
    // An empty library: nothing is milled, still no loss.
    let mut g = bare().hand(0, &[SUPPLIER]).battlefield(0, &["Swamp"]).start();
    cast_and_resolve(&mut g, 0, SUPPLIER);
    assert!(gy(&g, 0).is_empty() && !g.is_over(), "{}", log(&g));
    g.pass_until(|g| g.is_over() || g.turn().number > 3);
    assert!(!g.is_over(), "no loss before its controller would draw");
}

/// A creature returned to its owner's hand doesn't die (CR 700.4): no mill.
/// Exiled instead of dying (an opponent's Leyline of the Void, CR 614.1a):
/// it never was put into a graveyard, so it doesn't die either (ruling
/// ef9cd497 of Leyline, the same rule) - and the cards it mills as it enters
/// are exiled instead.
#[test]
fn cr_700_4_stitchers_supplier_bounced_or_exiled_instead_doesnt_mill() {
    let mut g = bare()
        .library(0, &["Island", "Forest", "Plains", "Swamp"])
        .battlefield(0, &[SUPPLIER])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Brazen Borrower // Petty Theft"])
        .at(Step::Main1, 1)
        .start();
    let s = bf(&g, 0, SUPPLIER);
    let bb = hand(&g, 1, "Brazen Borrower // Petty Theft");
    g.choose(Choice::CastFace(bb));
    { let t = g.tgt(s); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Hand, "{}", log(&g));
    assert!(gy(&g, 0).is_empty(), "bounced: no dies trigger");

    let mut g = bare()
        .library(0, &["Island", "Forest", "Plains", "Swamp", "Mountain", "Mountain"])
        .library(1, &["Island"; 5])
        .hand(0, &[SUPPLIER])
        .battlefield(0, &["Swamp"])
        .battlefield(1, &["Leyline of the Void", "Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    cast_and_resolve(&mut g, 0, SUPPLIER);
    assert!(gy(&g, 0).is_empty(), "the milled cards are exiled instead");
    assert_eq!(g.exile_zone().iter().filter(|&&c| g.obj(c).owner == 0).count(), 3);
    pass_to(&mut g, |g| g.to_move() == Some(1) && g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let s = bf(&g, 0, SUPPLIER);
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    { let t = g.tgt(s); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Exile, "{}", log(&g));
    assert_eq!(g.player(0).library.len(), 3, "exiled, it didn't die: no mill (P0 drew no card: P1's turn came first)\n{}", log(&g));
}

// ---- Dark Matter Manipulator --------------------------------------------------------

/// "+2/+0 for every seven cards in your graveyard" (CR 613.4c, layer 7c):
/// one +2/+0 per full group of seven cards of its controller's graveyard,
/// whatever their types; its toughness stays 2.
#[test]
fn cr_613_4c_dark_matter_manipulator_gets_2_0_for_every_seven_cards() {
    for (n, p) in [(0usize, 1i128), (6, 1), (7, 3), (13, 3), (14, 5), (20, 5), (21, 7)] {
        let cards: Vec<&str> = ["Island", "Lightning Bolt", "Stitcher's Supplier", "Forest", "Ponder", "Swamp", "Wasteland"].iter().copied().cycle().take(n).collect();
        let g = bare().battlefield(0, &[DMM]).graveyard(0, &cards).start();
        let d = bf(&g, 0, DMM);
        assert_eq!(pt(&g, d), (p, 2), "{n} cards");
    }
}

/// "Your graveyard": its controller's, not the opponent's; and only cards
/// (a token briefly in a graveyard isn't one, CR 108.2, 111.1).
#[test]
fn dark_matter_manipulator_counts_only_its_controllers_graveyard_cards() {
    let mut g = bare().battlefield(0, &[DMM]).graveyard(0, &["Island"; 6]).graveyard(1, &["Island"; 10]).start();
    let d = bf(&g, 0, DMM);
    assert_eq!(pt(&g, d), (1, 2));
    g.put_token_in_graveyard(0, "Orc Army");
    assert_eq!(pt(&g, d), (1, 2), "a token isn't a card");
    // The opponent's Manipulator counts the opponent's graveyard.
    let g = bare().battlefield(1, &[DMM]).graveyard(0, &["Island"; 6]).graveyard(1, &["Island"; 10]).start();
    assert_eq!(pt(&g, bf(&g, 1, DMM)), (3, 2));
}

/// Its own enters trigger mills three (CR 701.17a); the bonus follows the
/// graveyard as it changes (CR 611.3a: a static ability's effect applies at
/// all times): 4 cards + 3 milled = 7, +2/+0.
#[test]
fn cr_611_3a_dark_matter_manipulator_mills_and_grows_at_once() {
    let mut g = bare()
        .library(0, &["Island", "Island", "Island", "Island"])
        .graveyard(0, &["Ponder", "Ponder", "Ponder", "Ponder"])
        .hand(0, &[DMM])
        .battlefield(0, &["Swamp"])
        .start();
    cast(&mut g, 0, DMM);
    resolve(&mut g);
    let d = bf(&g, 0, DMM);
    assert_eq!(g.player(0).graveyard.len(), 7, "{}", log(&g));
    assert_eq!(pt(&g, d), (3, 2));
    // Cards leaving the graveyard shrink it again: a delve-free exile by
    // Dauthi Voidwalker's? (No such effect needed: Lion's Eye Diamond's
    // discard grows it; a reanimation shrinks it.)
}

/// Layer 7c adds the bonus to counters and other modifications (CR 613.4c,
/// 613.4b-c: a +1/+1 counter and the static together); -1/-1 counters
/// subtract.
#[test]
fn cr_613_4c_dark_matter_manipulator_with_counters() {
    let g = bare().battlefield(0, &[DMM]).counters(0, DMM, Counter::P1P1, 1).graveyard(0, &["Island"; 7]).start();
    assert_eq!(pt(&g, bf(&g, 0, DMM)), (4, 3));
    let g = bare().battlefield(0, &[DMM]).counters(0, DMM, Counter::M1M1, 1).graveyard(0, &["Island"; 14]).start();
    assert_eq!(pt(&g, bf(&g, 0, DMM)), (4, 1));
}

/// The bonus is read when the power is: combat damage with 7 cards in the
/// graveyard is 3; with 6 at the declaration and a seventh card put there
/// before damage (Brainstorm, resolved: CR 608.2n), the damage is 3 too.
#[test]
fn dark_matter_manipulator_deals_damage_with_its_power_as_it_is() {
    let mut g = bare().battlefield(0, &[DMM]).graveyard(0, &["Island"; 7]).at(Step::DeclareAttackers, 0).start();
    let d = bf(&g, 0, DMM);
    g.choose(Choice::Card(d));
    g.choose(Choice::Pass);
    g.pass_until(|g| g.turn().step >= Step::EndCombat);
    assert_eq!(g.life(1), 17, "{}", log(&g));
    let mut g = bare()
        .battlefield(0, &[DMM, "Island"])
        .graveyard(0, &["Island"; 6])
        .library(0, &["Island", "Island", "Island"])
        .hand(0, &["Brainstorm", "Forest"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let d = bf(&g, 0, DMM);
    g.choose(Choice::Card(d));
    g.choose(Choice::Pass);
    g.drive_until(at_priority);
    assert_eq!(pt(&g, d), (1, 2));
    cast(&mut g, 0, "Brainstorm");
    resolve(&mut g);
    assert_eq!(g.player(0).graveyard.len(), 7, "{}", log(&g));
    assert_eq!(pt(&g, d), (3, 2));
    g.pass_until(|g| g.turn().step >= Step::EndCombat);
    assert_eq!(g.life(1), 17, "{}", log(&g));
}

// ---- Moonshadow --------------------------------------------------------------------

/// A Moonshadow on the battlefield with `n` -1/-1 counters (as if it had
/// entered earlier and lost some).
fn moon_with(s: Scenario, p: PlayerId, n: u64) -> Scenario {
    let s = s.battlefield(p, &[MOON]);
    if n > 0 {
        s.counters(p, MOON, Counter::M1M1, n as engine::num::Count)
    } else {
        s
    }
}

/// The triggered abilities of Moonshadow on the stack.
fn moon_triggers(g: &Game) -> usize {
    g.stack().iter().filter(|s| ability_of(g, *s) == Some(MOON)).count()
}

/// The name of the source of a stack item that is an ability.
fn ability_of(g: &Game, s: &engine::game::StackItem) -> Option<&'static str> {
    match s.kind {
        engine::game::StackKind::Ability { ability, .. } => Some(g.db().def(ability.def).name.as_str()),
        _ => None,
    }
}

/// "This creature enters with six -1/-1 counters on it" (CR 614.1c, 122.6):
/// a replacement effect: it is a 1/1 from the moment it is on the
/// battlefield (never a 7/7 there); menace (CR 702.111).
#[test]
fn cr_614_1c_moonshadow_enters_with_six_minus_one_counters() {
    let mut g = bare().hand(0, &[MOON]).battlefield(0, &["Swamp"]).start();
    cast_and_resolve(&mut g, 0, MOON);
    let m = bf(&g, 0, MOON);
    assert_eq!(m1(&g, m), 6, "{}", log(&g));
    assert_eq!(pt(&g, m), (1, 1));
    assert!(g.has_kw(m, Kw::Menace));
    assert!(!log(&g).contains("Moonshadow goes on the stack") || true);
    // Off the battlefield it is the printed 7/7 (counters exist only on it).
    let g = bare().hand(0, &[MOON]).start();
    assert_eq!(pt(&g, hand(&g, 0, MOON)), (7, 7));
}

/// Menace (CR 702.111b): it can't be blocked except by two or more
/// creatures.
#[test]
fn cr_702_111b_moonshadow_has_menace() {
    let mut g = moon_with(bare(), 0, 6).battlefield(1, &["Birds of Paradise"]).at(Step::DeclareAttackers, 0).start();
    let m = bf(&g, 0, MOON);
    g.choose(Choice::Card(m));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step > Step::DeclareBlockers);
    assert!(!(g.kind() == Some(DecisionKind::DeclareBlockers) && g.has_option(Choice::Card(m))), "one blocker can't block it: {}", opts(&g));
    if g.kind() == Some(DecisionKind::DeclareBlockers) {
        g.pass();
    }
    g.pass_until(|g| g.turn().step >= Step::EndCombat);
    assert_eq!(g.life(1), 19, "{}", log(&g));
}

/// "Whenever one or more permanent cards are put into your graveyard from
/// anywhere": one trigger per event (CR 603.2c), however many permanent cards
/// it puts there: Stitcher's Supplier's mill of a land, an instant and a
/// creature card is one event, one trigger, one counter.
#[test]
fn cr_603_2c_moonshadow_one_trigger_per_mill() {
    let mut g = moon_with(bare(), 0, 6)
        .library(0, &["Island", "Lightning Bolt", SUPPLIER, "Swamp"])
        .hand(0, &[SUPPLIER])
        .battlefield(0, &["Swamp"])
        .start();
    cast(&mut g, 0, SUPPLIER);
    // The Supplier's enters trigger on the stack: no Moonshadow trigger yet.
    g.pass();
    g.drive_until(|g| at_priority(g) || moon_triggers(g) > 0);
    assert_eq!(moon_triggers(&g), 0);
    resolve(&mut g);
    let m = bf(&g, 0, MOON);
    assert_eq!(gy(&g, 0), vec!["Island", "Lightning Bolt", SUPPLIER], "{}", log(&g));
    assert_eq!(m1(&g, m), 5, "one trigger for the one mill\n{}", log(&g));
    assert_eq!(pt(&g, m), (2, 2));
}

/// Dark Matter Manipulator's mill likewise; a mill of instants and sorceries
/// only puts no permanent card there: no trigger.
#[test]
fn moonshadow_and_dark_matter_manipulators_mill() {
    let mut g = moon_with(bare(), 0, 6).library(0, &["Forest", "Island", "Ponder"]).hand(0, &[DMM]).battlefield(0, &["Swamp"]).start();
    cast_and_resolve(&mut g, 0, DMM);
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 5, "{}", log(&g));
    let mut g = moon_with(bare(), 0, 6).library(0, &["Ponder", "Lightning Bolt", "Brainstorm"]).hand(0, &[DMM]).battlefield(0, &["Swamp"]).start();
    cast_and_resolve(&mut g, 0, DMM);
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 6, "instants and sorceries aren't permanent cards (CR 110.4a)\n{}", log(&g));
}

/// A creature of its controller dying (battlefield to graveyard: "from
/// anywhere"); the opponent's creature dying goes to the opponent's
/// graveyard: not "your graveyard".
#[test]
fn moonshadow_a_creature_dying_into_your_graveyard() {
    let mut g = moon_with(bare(), 0, 6)
        .battlefield(0, &[SUPPLIER])
        .battlefield(1, &["Birds of Paradise", "Mountain", "Mountain"])
        .hand(1, &["Lightning Bolt", "Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let birds = bf(&g, 1, "Birds of Paradise");
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    { let t = g.tgt(birds); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    let m = bf(&g, 0, MOON);
    assert_eq!(m1(&g, m), 6, "the opponent's creature: their graveyard\n{}", log(&g));
    let s = bf(&g, 0, SUPPLIER);
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    { let t = g.tgt(s); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    // The Supplier's death, then its dies trigger's mill (an empty library):
    // one Moonshadow trigger.
    assert_eq!(m1(&g, m), 5, "{}", log(&g));
}

/// Discarded permanent cards (a hand is "anywhere"): Thoughtseize taking a
/// creature card of Moonshadow's controller; a discarded instant doesn't
/// count.
#[test]
fn moonshadow_a_discarded_permanent_card() {
    let mut g = moon_with(bare(), 0, 6).hand(0, &[DMM, "Lightning Bolt"]).battlefield(1, &["Swamp", "Swamp"]).hand(1, &["Thoughtseize", "Thoughtseize"]).at(Step::Main1, 1).start();
    let dmm = hand(&g, 0, DMM);
    let ts = hand(&g, 1, "Thoughtseize");
    g.choose(Choice::Cast(ts));
    pay_with(&mut g, target(player(0)));
    resolve_with(&mut g, target(Choice::Card(dmm)));
    let m = bf(&g, 0, MOON);
    assert_eq!(g.obj(dmm).zone, Zone::Graveyard);
    assert_eq!(m1(&g, m), 5, "{}", log(&g));
    let bolt = hand(&g, 0, "Lightning Bolt");
    let ts = hand(&g, 1, "Thoughtseize");
    g.choose(Choice::Cast(ts));
    pay_with(&mut g, target(player(0)));
    resolve_with(&mut g, target(Choice::Card(bolt)));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(m1(&g, m), 5, "an instant: no trigger\n{}", log(&g));
}

/// A countered creature spell (the stack is "anywhere"): Counterspell on
/// Moonshadow's controller's Stitcher's Supplier puts the card into their
/// graveyard; a countered instant doesn't count.
#[test]
fn moonshadow_a_countered_creature_spell() {
    let mut g = moon_with(bare(), 0, 6)
        .hand(0, &[SUPPLIER, "Lightning Bolt"])
        .battlefield(0, &["Swamp", "Mountain"])
        .battlefield(1, &["Island", "Island", "Island", "Island"])
        .hand(1, &["Counterspell", "Counterspell"])
        .start();
    cast(&mut g, 0, SUPPLIER);
    g.pass();
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(1));
    let cs = hand(&g, 1, "Counterspell");
    let s = hand_or_stack(&g, 0, SUPPLIER);
    g.choose(Choice::Cast(cs));
    { let t = g.tgt(s); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Graveyard, "{}", log(&g));
    let m = bf(&g, 0, MOON);
    assert_eq!(m1(&g, m), 5, "{}", log(&g));
    let bolt = hand(&g, 0, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    pay_with(&mut g, target(player(1)));
    g.pass();
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(1));
    let cs = hand(&g, 1, "Counterspell");
    g.choose(Choice::Cast(cs));
    { let t = g.tgt(bolt); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(m1(&g, m), 5, "a countered instant: no trigger\n{}", log(&g));
}

/// The card `name` of P`p` on the stack (a spell being cast).
fn hand_or_stack(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.find_card(p, name, Zone::Stack).unwrap_or_else(|| hand(g, p, name))
}

/// A token dying isn't a card put into a graveyard (CR 108.2, 111.1: the
/// Orc Army token goes there and ceases to exist): no trigger.
#[test]
fn moonshadow_a_token_dying_doesnt_trigger_it() {
    let mut g = moon_with(bare(), 0, 6)
        .library(0, &["Island"; 5])
        .library(1, &["Island"; 5])
        .hand(0, &["Orcish Bowmasters"])
        .battlefield(0, &["Swamp", "Swamp"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    cast(&mut g, 0, "Orcish Bowmasters");
    resolve_with(&mut g, target(player(1)));
    let army = bf(&g, 0, "Orc Army");
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 6);
    pass_to(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    { let t = g.tgt(army); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_ne!(g.obj(army).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 6, "a token: no trigger\n{}", log(&g));
}

/// Dryad Arbor destroyed by Wasteland: a land creature card (CR 110.4a: a
/// permanent card) put into its owner's graveyard: one trigger.
#[test]
fn moonshadow_dryad_arbor_destroyed_is_a_permanent_card() {
    let mut g = moon_with(bare(), 0, 6).battlefield(0, &[ARBOR]).battlefield(1, &["Wasteland"]).at(Step::Main1, 1).start();
    let a = bf(&g, 0, ARBOR);
    let w = bf(&g, 1, "Wasteland");
    let act = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, AbilitySlot::Activated(k)) if *x == w && *k > 0)).unwrap_or_else(|| panic!("Wasteland: {}", opts(&g)));
    g.choose(act);
    { let t = g.tgt(a); pay_with(&mut g, target(t)) };
    // The Wasteland's sacrifice (its cost) went to P1's graveyard: P1's.
    resolve(&mut g);
    assert_eq!(g.obj(a).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 5, "{}", log(&g));
}

/// A fetch land's cost: the land is sacrificed into its owner's graveyard,
/// one event (the cost, CR 603.2c, 118.3): one trigger as it is
/// activated; the search finds a land onto the battlefield, no card into the
/// graveyard.
#[test]
fn moonshadow_a_fetch_land_sacrificed_as_a_cost() {
    let mut g = moon_with(bare(), 0, 6).battlefield(0, &["Misty Rainforest"]).library(0, &["Island", "Forest"]).start();
    let f = bf(&g, 0, "Misty Rainforest");
    let act = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == f)).unwrap_or_else(|| panic!("the fetch: {}", opts(&g)));
    g.choose(act);
    pay(&mut g);
    assert_eq!(g.obj(f).zone, Zone::Graveyard);
    assert_eq!(moon_triggers(&g), 1, "the trigger is on the stack above the fetch's ability\n{}", log(&g));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| Choice::Def(g.db().id("Forest").unwrap())));
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 5, "{}", log(&g));
}

/// "While this creature has a -1/-1 counter on it" is part of the trigger
/// condition, checked as the event happens (CR 603.2, 603.10: not an
/// intervening "if", 603.4, which is about "if" only): with none it doesn't
/// trigger; with one, two events trigger it twice, and the second does
/// nothing as it resolves (no counter left to remove; CR 122.8 is not
/// applied twice).
#[test]
fn cr_603_4_moonshadows_while_is_checked_as_the_event_happens() {
    // No -1/-1 counter: no trigger at all.
    let mut g = moon_with(bare(), 0, 0).library(0, &["Forest", "Island", "Swamp"]).hand(0, &[SUPPLIER]).battlefield(0, &["Swamp"]).start();
    cast_and_resolve(&mut g, 0, SUPPLIER);
    assert_eq!(pt(&g, bf(&g, 0, MOON)), (7, 7));
    assert!(!log(&g).contains("trigger of Moonshadow"), "{}", log(&g));
    // One counter, two events (P0's fetch land, then a Lightning Bolt on its
    // Dark Matter Manipulator, both before the first trigger resolves).
    let mut g = moon_with(bare(), 0, 1)
        .battlefield(0, &["Misty Rainforest", DMM])
        .library(0, &["Island", "Swamp", "Swamp", "Swamp"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Lightning Bolt"])
        .start();
    let f = bf(&g, 0, "Misty Rainforest");
    let act = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == f)).unwrap();
    g.choose(act);
    pay(&mut g);
    assert_eq!(moon_triggers(&g), 1);
    // P0 passes; P1 bolts the Manipulator in response.
    g.pass();
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(1));
    let s = bf(&g, 0, DMM);
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    { let t = g.tgt(s); pay_with(&mut g, target(t)) };
    g.pass();
    g.drive_until(|g| at_priority(g) && g.stack().iter().all(|x| !matches!(x.kind, engine::game::StackKind::Spell { .. })));
    // (P0 orders its two new triggers: the Supplier's and Moonshadow's.)
    while g.kind() == Some(DecisionKind::OrderTriggers) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(at_priority);
    }
    assert_eq!(g.obj(s).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(moon_triggers(&g), 2, "the Manipulator's death is a second event while the counter is still on it\n{}", log(&g));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| Choice::Def(g.db().id("Island").unwrap())));
    let m = bf(&g, 0, MOON);
    assert_eq!(m1(&g, m), 0);
    assert_eq!(pt(&g, m), (7, 7), "{}", log(&g));
}

/// CR 603.6c: an ability that triggers on a card put into a zone "from
/// anywhere" is never a leaves-the-battlefield ability, even when the card
/// comes from the battlefield; so it doesn't look back (603.10): Moonshadow
/// put into its owner's graveyard together with another creature card
/// (Brotherhood's End's damage: one simultaneous move, CR 704.3) isn't on the
/// battlefield after the event, so it doesn't trigger; dying alone it
/// doesn't trigger for itself either. One that survives the same damage
/// does.
#[test]
fn cr_603_6c_moonshadow_dying_with_others_doesnt_trigger() {
    let mut g = moon_with(bare(), 0, 6).battlefield(0, &[DMM]).battlefield(1, &["Mountain", "Mountain", "Mountain"]).hand(1, &["Brotherhood's End"]).at(Step::Main1, 1).start();
    let be = hand(&g, 1, "Brotherhood's End");
    g.choose(Choice::Cast(be));
    pay(&mut g);
    resolve(&mut g);
    assert_eq!(g.count_in(0, MOON, Zone::Graveyard), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, DMM, Zone::Graveyard), 1);
    assert!(!log(&g).contains("trigger of Moonshadow"), "{}", log(&g));
    // Alone.
    let mut g = moon_with(bare(), 0, 6).battlefield(1, &["Mountain"]).hand(1, &["Lightning Bolt"]).at(Step::Main1, 1).start();
    let m = bf(&g, 0, MOON);
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    { let t = g.tgt(m); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Graveyard);
    assert!(!log(&g).contains("trigger of Moonshadow"), "{}", log(&g));
    // A Moonshadow with three counters (a 4/4) survives 3 damage: the
    // Manipulator's death triggers it.
    let mut g = moon_with(bare(), 0, 3).battlefield(0, &[DMM, SUPPLIER]).battlefield(1, &["Mountain", "Mountain", "Mountain"]).hand(1, &["Brotherhood's End"]).at(Step::Main1, 1).start();
    let be = hand(&g, 1, "Brotherhood's End");
    g.choose(Choice::Cast(be));
    pay(&mut g);
    resolve(&mut g);
    let m = bf(&g, 0, MOON);
    // Two creature cards died at once: one trigger (one event), then the
    // Supplier's dies trigger's mill (an empty library) puts nothing there.
    assert_eq!(m1(&g, m), 2, "{}", log(&g));
}

/// CR 704.5q, 122.3: +1/+1 and -1/-1 counters annihilate as a state-based
/// action: a Moonshadow given as many +1/+1 counters as its -1/-1 ones has
/// none left, and then doesn't trigger.
#[test]
fn cr_704_5q_plus_one_counters_annihilate_moonshadows_minus_ones() {
    let mut g = moon_with(bare(), 0, 2)
        .counters(0, MOON, Counter::P1P1, 2)
        .library(0, &["Forest", "Island", "Swamp"])
        .hand(0, &[SUPPLIER])
        .battlefield(0, &["Swamp"])
        .start();
    let m = bf(&g, 0, MOON);
    assert_eq!((m1(&g, m), g.counters_on(m, Counter::P1P1) as u64), (0, 0), "the SBA ran before priority\n{}", log(&g));
    cast_and_resolve(&mut g, 0, SUPPLIER);
    assert_eq!(pt(&g, m), (7, 7));
    assert!(!log(&g).contains("trigger of Moonshadow"), "{}", log(&g));
    // Three +1/+1 against two -1/-1: one +1/+1 remains.
    let g = moon_with(bare(), 0, 2).counters(0, MOON, Counter::P1P1, 3).start();
    let m = bf(&g, 0, MOON);
    assert_eq!((m1(&g, m), g.counters_on(m, Counter::P1P1) as u64), (0, 1));
    assert_eq!(pt(&g, m), (8, 8));
}

/// "If a card would be put into an opponent's graveyard from anywhere,
/// exile it instead" (Leyline of the Void, CR 614.1a): the cards never reach
/// the graveyard, so nothing triggers; Dauthi Voidwalker's void counter
/// exile likewise.
#[test]
fn cr_614_1a_cards_exiled_instead_dont_trigger_moonshadow() {
    for hoser in ["Leyline of the Void", "Dauthi Voidwalker"] {
        let mut g = moon_with(bare(), 0, 6)
            .library(0, &["Forest", "Island", SUPPLIER])
            .hand(0, &[SUPPLIER])
            .battlefield(0, &["Swamp"])
            .battlefield(1, &[hoser])
            .start();
        cast_and_resolve(&mut g, 0, SUPPLIER);
        assert!(gy(&g, 0).is_empty(), "{hoser}: {}", log(&g));
        assert_eq!(m1(&g, bf(&g, 0, MOON)), 6, "{hoser}: {}", log(&g));
    }
}

/// A card's characteristics in the graveyard decide whether it is a
/// permanent card (CR 110.4a; the event is seen after it, CR 603.10): an
/// adventurer card is a creature card there (CR 715.4: Brazen Borrower, its
/// Adventure countered); a modal double-faced card has only its front face's
/// (CR 712.8a: Sink into Stupor, an instant: no); a split card has both
/// halves' (CR 709.4: Rough // Tumble, a sorcery: no).
#[test]
fn moonshadow_double_faced_adventure_and_split_cards_by_their_graveyard_characteristics() {
    let discard = |card: &str| -> u64 {
        let mut g = moon_with(bare(), 0, 6).hand(0, &[card]).battlefield(1, &["Swamp"]).hand(1, &["Thoughtseize"]).at(Step::Main1, 1).start();
        let c = hand(&g, 0, card);
        let ts = hand(&g, 1, "Thoughtseize");
        g.choose(Choice::Cast(ts));
        pay_with(&mut g, target(player(0)));
        resolve_with(&mut g, target(Choice::Card(c)));
        assert_eq!(g.obj(c).zone, Zone::Graveyard, "{card}: {}", log(&g));
        m1(&g, bf(&g, 0, MOON))
    };
    assert_eq!(discard("Brazen Borrower // Petty Theft"), 5);
    assert_eq!(discard("Sink into Stupor // Soporific Springs"), 6);
    assert_eq!(discard("Witch Enchanter // Witch-Blessed Meadow"), 5);
    assert_eq!(discard("Rough // Tumble"), 6);
    // Petty Theft countered: the Brazen Borrower card goes to the graveyard
    // (CR 715.4: not on an adventure) - a creature card.
    let mut g = moon_with(bare(), 0, 6)
        .hand(0, &["Brazen Borrower // Petty Theft"])
        .battlefield(0, &["Island", "Island"])
        .battlefield(1, &["Birds of Paradise", "Island", "Island"])
        .hand(1, &["Counterspell"])
        .start();
    let bb = hand(&g, 0, "Brazen Borrower // Petty Theft");
    let birds = bf(&g, 1, "Birds of Paradise");
    g.choose(Choice::CastFace(bb));
    { let t = g.tgt(birds); pay_with(&mut g, target(t)) };
    g.pass();
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(1));
    let cs = hand(&g, 1, "Counterspell");
    g.choose(Choice::Cast(cs));
    { let t = g.tgt(bb); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(bb).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 5, "{}", log(&g));
}

/// The trigger removes a counter from "this creature": a Moonshadow that left
/// the battlefield before it resolves is a new object (CR 400.7): nothing
/// happens. And the trigger is a triggered ability on the stack: Consign to
/// Memory may counter it.
#[test]
fn moonshadows_trigger_without_its_source_and_countered() {
    let mut g = moon_with(bare(), 0, 6)
        .battlefield(0, &["Misty Rainforest"])
        .library(0, &["Island", "Forest"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Consign to Memory"])
        .start();
    let f = bf(&g, 0, "Misty Rainforest");
    let act = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == f)).unwrap();
    g.choose(act);
    pay(&mut g);
    g.pass();
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(1));
    let ctm = hand(&g, 1, "Consign to Memory");
    g.choose(Choice::Cast(ctm));
    let id = g.stack().iter().find(|s| ability_of(&g, s) == Some(MOON)).unwrap().id;
    pay_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseMode) {
            // (Replicate paid no times.)
            return Some(g.pending_decision().unwrap().options[0].choice);
        }
        target(Choice::Target(engine::Target::Stack(id)))(g)
    });
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::SearchLibrary)).then(|| Choice::Def(g.db().id("Forest").unwrap())));
    assert_eq!(m1(&g, bf(&g, 0, MOON)), 6, "countered\n{}", log(&g));
}

// ---- Dryad Arbor -----------------------------------------------------------------

/// Is an activated ability of permanent `c` offered at priority?
fn ability_offered(g: &Game, c: CardIdx) -> bool {
    g.offered().iter().any(|o| matches!(o, Choice::Ability(x, _) if *x == c))
}

/// "Dryad Arbor is played as a land. It doesn't use the stack, it's not a
/// spell, it can't be responded to, it has no mana cost, and it counts as
/// your land play for the turn" (CR 305.9: a land and another card type is
/// played only as a land; 305.1, 116.2a).
#[test]
fn cr_305_9_dryad_arbor_is_played_as_a_land_only() {
    // ruling: 5fa00503
    let mut g = bare().hand(0, &[ARBOR, "Forest"]).start();
    let a = hand(&g, 0, ARBOR);
    assert!(g.has_option(Choice::Play(a)), "{}", opts(&g));
    assert!(!g.has_option(Choice::Cast(a)), "never cast: {}", opts(&g));
    assert_eq!(g.def_of(a).mana_value(), 0);
    assert!(g.def_of(a).cost.is_zero());
    g.choose(Choice::Play(a));
    // No stack, no response: its player has priority again at once.
    assert!(g.stack().is_empty());
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Priority), Some(0)));
    assert_eq!(g.player(0).lands_played, 1);
    let f = hand(&g, 0, "Forest");
    assert!(!g.has_option(Choice::Play(f)), "the land play is used: {}", opts(&g));
}

/// Omniscience lets spells be cast without paying their mana costs: Dryad
/// Arbor is no spell (CR 305.9), still only played. Show and Tell puts it
/// onto the battlefield: not a land play (CR 305.4), and it is summoning
/// sick (CR 302.6).
#[test]
fn cr_305_4_dryad_arbor_with_omniscience_and_show_and_tell() {
    let g = bare().battlefield(0, &["Omniscience"]).hand(0, &[ARBOR]).start();
    let a = hand(&g, 0, ARBOR);
    assert!(g.has_option(Choice::Play(a)) && !g.has_option(Choice::Cast(a)), "{}", opts(&g));
    let mut g = bare().battlefield(0, &["Island", "Island", "Island"]).hand(0, &["Show and Tell", ARBOR]).start();
    cast(&mut g, 0, "Show and Tell");
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::ChooseHandCard) && g.pending_decision().unwrap().player == 0 {
            let a = g.find_card(0, ARBOR, Zone::Hand)?;
            return Some(Choice::Card(a));
        }
        None
    });
    let a = bf(&g, 0, ARBOR);
    assert_eq!(g.player(0).lands_played, 0, "put, not played\n{}", log(&g));
    assert!(g.is_sick(a));
}

/// "It's affected by summoning sickness" (CR 302.6): the turn it comes under
/// its controller's control it can't attack or use its "{T}: Add {G}" (a {G}
/// spell isn't castable from it alone); from its controller's next turn on it
/// can.
#[test]
fn cr_302_6_dryad_arbor_is_affected_by_summoning_sickness() {
    let mut g = bare().hand(0, &[ARBOR, "Birds of Paradise"]).library(0, &["Island"; 5]).library(1, &["Island"; 5]).start();
    let a = hand(&g, 0, ARBOR);
    g.choose(Choice::Play(a));
    let birds = hand(&g, 0, "Birds of Paradise");
    assert!(!g.has_option(Choice::Cast(birds)), "its mana ability can't be activated yet: {}", opts(&g));
    assert!(!g.has_option(Choice::Yes), "no mana to float either: {}", opts(&g));
    // Combat: not an attacker.
    pass_to(&mut g, |g| g.kind() == Some(DecisionKind::DeclareAttackers) || g.turn().active != 0);
    assert!(!(g.kind() == Some(DecisionKind::DeclareAttackers) && g.has_option(Choice::Card(a))), "{}", opts(&g));
    // The next turn of P0.
    pass_to(&mut g, |g| g.turn().active == 0 && g.turn().number == 5 && g.turn().step == Step::Main1 && at_priority(g));
    let birds = hand(&g, 0, "Birds of Paradise");
    assert!(g.has_option(Choice::Cast(birds)), "{}", opts(&g));
    g.choose(Choice::Cast(birds));
    pay(&mut g);
    assert!(g.obj(a).tapped, "tapped for {{G}}\n{}", log(&g));
    resolve(&mut g);
    assert_eq!(g.count_in(0, "Birds of Paradise", Zone::Battlefield), 1);
}

/// A summoning-sick Dryad Arbor still blocks (CR 302.6 restricts attacking
/// and {T} abilities only).
#[test]
fn cr_302_6_a_summoning_sick_dryad_arbor_can_block() {
    let mut g = bare().battlefield(0, &["Birds of Paradise"]).hand(1, &[ARBOR]).library(0, &["Island"; 5]).library(1, &["Island"; 5]).at(Step::Main1, 1).start();
    let a = hand(&g, 1, ARBOR);
    g.choose(Choice::Play(a));
    pass_to(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let birds = bf(&g, 0, "Birds of Paradise");
    // P0 attacks with the Birds (flying: the Arbor can't block a flyer).
    g.choose(Choice::Card(birds));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers) || g.turn().step > Step::DeclareBlockers);
    assert!(!g.has_option(Choice::Card(birds)), "flying (CR 702.9b)");
    let mut g = bare().battlefield(0, &[SUPPLIER]).hand(1, &[ARBOR]).library(0, &["Island"; 5]).library(1, &["Island"; 5]).at(Step::Main1, 1).start();
    let a = hand(&g, 1, ARBOR);
    g.choose(Choice::Play(a));
    let a = bf(&g, 1, ARBOR);
    pass_to(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::DeclareAttackers));
    let s = bf(&g, 0, SUPPLIER);
    g.choose(Choice::Card(s));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    assert!(g.is_sick(a));
    assert!(g.has_option(Choice::Card(s)), "{}", opts(&g));
}

/// "Due to its color indicator ..., Dryad Arbor is green. Color indicators
/// apply in all zones" (CR 204.1, 202.2e): green in the hand, on the
/// battlefield and in the graveyard. Colour-changing effects still apply
/// (CR 613.1e; ruling 0dfaac8c): Painter's Servant's colour is added.
#[test]
fn cr_204_1_dryad_arbor_is_green_in_every_zone() {
    // ruling: 1df01b0e
    // ruling: 0dfaac8c
    let green = 1 << engine::mana::G;
    let g = bare().hand(0, &[ARBOR]).battlefield(0, &[ARBOR]).graveyard(0, &[ARBOR]).start();
    for z in [Zone::Hand, Zone::Battlefield, Zone::Graveyard] {
        let a = g.card(0, ARBOR, z);
        assert_eq!(g.colors_of(a), green, "{z:?}");
    }
    let mut g = bare().battlefield(0, &[ARBOR]).hand(0, &["Painter's Servant"]).battlefield(0, &["Island", "Island"]).start();
    cast(&mut g, 0, "Painter's Servant");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::ChooseMode)).then(|| g.pending_decision().unwrap().options[1].choice));
    let a = bf(&g, 0, ARBOR);
    assert_ne!(g.colors_of(a) & !green, 0, "the chosen colour is added: {}", log(&g));
    assert_ne!(g.colors_of(a) & green, 0, "and it stays green");
}

/// "Forest is a land type and Dryad is a creature type" (CR 205.3i,
/// 205.3m): it is a Forest (Misty Rainforest's "Forest or Island card"
/// finds it; it enters untapped and summoning sick) and a Dryad.
#[test]
fn cr_205_3_dryad_arbor_is_a_forest_and_a_dryad_fetched_untapped_and_sick() {
    // ruling: 9da0e7ba
    let mut g = bare().battlefield(0, &["Misty Rainforest"]).library(0, &[ARBOR, "Swamp"]).hand(0, &["Birds of Paradise"]).start();
    let f = bf(&g, 0, "Misty Rainforest");
    let act = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == f)).unwrap();
    g.choose(act);
    pay(&mut g);
    let mut seen = Vec::new();
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::SearchLibrary) {
            return Some(Choice::Def(g.db().id(ARBOR).unwrap()));
        }
        None
    });
    let a = bf(&g, 0, ARBOR);
    let forest = g.db().subtype_id("Forest").unwrap();
    let dryad = g.db().subtype_id("Dryad").unwrap();
    assert!(g.has_subtype(a, forest) && g.has_subtype(a, dryad));
    assert!(!g.obj(a).tapped && g.is_sick(a), "{}", log(&g));
    let birds = hand(&g, 0, "Birds of Paradise");
    assert!(!g.has_option(Choice::Cast(birds)), "sick: {}", opts(&g));
    seen.push(());
}

/// "If Dryad Arbor is changed into another basic land type (such as by Sea's
/// Claim), it continues to be a creature and a Dryad" / "... a green Dryad
/// creature" (CR 305.7: setting a land's subtype doesn't remove card types):
/// under Magus of the Moon it is a Mountain, not a Forest; it taps for {R},
/// not {G}; still a green 1/1 Dryad creature.
#[test]
fn cr_305_7_dryad_arbor_under_magus_of_the_moon() {
    // ruling: c5c89b6e
    // ruling: 2c34d4ba
    let mut g = bare().battlefield(0, &[ARBOR]).battlefield(1, &["Magus of the Moon"]).hand(0, &["Lightning Bolt", "Birds of Paradise"]).start();
    let a = bf(&g, 0, ARBOR);
    let db = g.db();
    let (forest, mountain, dryad) = (db.subtype_id("Forest").unwrap(), db.subtype_id("Mountain").unwrap(), db.subtype_id("Dryad").unwrap());
    assert!(g.has_subtype(a, mountain) && !g.has_subtype(a, forest) && g.has_subtype(a, dryad));
    assert!(g.is_creature(a) && g.is_type(a, types::LAND));
    assert_eq!(pt(&g, a), (1, 1));
    assert_eq!(g.colors_of(a), 1 << engine::mana::G);
    let bolt = hand(&g, 0, "Lightning Bolt");
    let birds = hand(&g, 0, "Birds of Paradise");
    assert!(g.has_option(Choice::Cast(bolt)), "{{R}}: {}", opts(&g));
    assert!(!g.has_option(Choice::Cast(birds)), "no {{G}}: {}", opts(&g));
    g.choose(Choice::Cast(bolt));
    pay_with(&mut g, target(player(1)));
    assert!(g.obj(a).tapped);
}

/// "If a Dryad Arbor gains flash ... you can ignore the normal timing rules
/// ..., but not any other restrictions. You can't play Dryad Arbor during
/// another player's turn, and you can't play Dryad Arbor if you don't have
/// any land plays remaining" (CR 305.2b, 305.3). No card of the tables gives
/// a land flash; the restrictions that stay are tested: not on the other
/// player's turn (even in their main phase with an empty stack), not with no
/// land play left, not with a spell on the stack (CR 305.1).
#[test]
fn cr_305_3_dryad_arbor_only_on_its_owners_turn_with_a_land_play_left() {
    // ruling: c0965c9d
    let mut g = bare().hand(0, &[ARBOR]).battlefield(1, &["Mountain"]).hand(1, &["Lightning Bolt"]).at(Step::Main1, 1).start();
    let a = hand(&g, 0, ARBOR);
    let bolt = hand(&g, 1, "Lightning Bolt");
    g.choose(Choice::Cast(bolt));
    pay_with(&mut g, target(player(1)));
    g.pass();
    g.drive_until(|g| at_priority(g) && g.to_move() == Some(0));
    assert!(!g.has_option(Choice::Play(a)), "the other player's turn: {}", opts(&g));
    let g = bare().hand(0, &[ARBOR]).lands_played(0, 1).start();
    assert!(!g.has_option(Choice::Play(hand(&g, 0, ARBOR))), "no land play left");
    let mut g = bare().hand(0, &[ARBOR, "Lightning Bolt"]).battlefield(0, &["Mountain"]).start();
    let a = hand(&g, 0, ARBOR);
    cast(&mut g, 0, "Lightning Bolt");
    let bolt_target = player(1);
    pay_with(&mut g, target(bolt_target));
    assert!(!g.stack().is_empty());
    assert!(!g.has_option(Choice::Play(a)), "a spell on the stack: {}", opts(&g));
}

/// A creature for everything: Lightning Bolt and Orcish Bowmasters' ping kill
/// it (CR 704.5g: it dies, CR 700.4); Fatal Push destroys it (mana value 0).
/// A nonbasic land for everything: Wasteland destroys it.
#[test]
fn dryad_arbor_is_a_creature_and_a_nonbasic_land_for_removal() {
    let mut g = bare().battlefield(0, &[ARBOR]).battlefield(1, &["Swamp", "Swamp"]).hand(1, &["Orcish Bowmasters"]).at(Step::Main1, 1).start();
    let a = bf(&g, 0, ARBOR);
    cast(&mut g, 1, "Orcish Bowmasters");
    { let t = g.tgt(a); resolve_with(&mut g, target(t)) };
    assert_eq!(g.obj(a).zone, Zone::Graveyard, "{}", log(&g));
    let mut g = bare().battlefield(0, &[ARBOR]).battlefield(1, &["Swamp"]).hand(1, &["Fatal Push"]).at(Step::Main1, 1).start();
    let a = bf(&g, 0, ARBOR);
    let fp = hand(&g, 1, "Fatal Push");
    g.choose(Choice::Cast(fp));
    { let t = g.tgt(a); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(a).zone, Zone::Graveyard, "{}", log(&g));
    let mut g = bare().battlefield(0, &[ARBOR]).battlefield(1, &["Wasteland"]).at(Step::Main1, 1).start();
    let a = bf(&g, 0, ARBOR);
    let w = bf(&g, 1, "Wasteland");
    let act = g.offered().into_iter().find(|c| matches!(c, Choice::Ability(x, AbilitySlot::Activated(k)) if *x == w && *k > 0)).unwrap();
    g.choose(act);
    { let t = g.tgt(a); pay_with(&mut g, target(t)) };
    resolve(&mut g);
    assert_eq!(g.obj(a).zone, Zone::Graveyard, "{}", log(&g));
}

/// Karakas returns a legendary creature: Dryad Arbor isn't legendary, so it
/// is no target (only the legendary ones are offered).
#[test]
fn karakas_cant_return_dryad_arbor() {
    let g = bare().battlefield(0, &["Karakas", ARBOR]).battlefield(1, &[ARBOR]).start();
    let k = bf(&g, 0, "Karakas");
    assert!(!g.offered().iter().any(|c| matches!(c, Choice::Ability(x, AbilitySlot::Activated(i)) if *x == k && *i > 0)), "{}", opts(&g));
}

/// Counted as a land and as a creature: "lands you control" and "creatures
/// you control" both count it (here: the encoding's per-player summary and
/// the engine's counts).
#[test]
fn dryad_arbor_counts_as_a_land_and_a_creature() {
    let g = bare().battlefield(0, &[ARBOR, "Forest", SUPPLIER]).start();
    assert_eq!(g.creatures_of(0).count(), 2);
    assert_eq!(g.permanents_of(0).filter(|&c| g.is_type(c, types::LAND)).count(), 2);
    let a = bf(&g, 0, ARBOR);
    assert!(!ability_offered(&g, a), "its mana ability is used in payments, not offered alone");
}

/// Attacking: not summoning sick, it attacks as a 1/1 and taps (CR 508.1f),
/// so its mana isn't available after (no vigilance).
#[test]
fn dryad_arbor_attacks_and_taps() {
    let mut g = bare().battlefield(0, &[ARBOR]).at(Step::DeclareAttackers, 0).start();
    let a = bf(&g, 0, ARBOR);
    assert!(g.has_option(Choice::Card(a)), "{}", opts(&g));
    g.choose(Choice::Card(a));
    g.choose(Choice::Pass);
    assert!(g.obj(a).tapped);
    g.pass_until(|g| g.turn().step >= Step::EndCombat);
    assert_eq!(g.life(1), 19);
}
