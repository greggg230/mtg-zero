//! Last-known mana value of a departed copy (ENGINE.md 223): an effect that
//! reads the mana value of an object that left its zone uses the object as it
//! last existed there (CR 608.2h, 113.7a), copy effects included: a copy of
//! the back face of a nonmodal double-faced permanent has mana value 0 (CR
//! 202.3b), a copy of an {X} creature counts X as 0 (202.3e), a spell's X
//! counts while it was a spell. Terminal Velocity's granted "When this
//! permanent leaves the battlefield, it deals damage equal to its mana value
//! to each creature" and Thor's "that spell's mana value" read them.
//!
//! The table: the cube's and the legacy pool's cards (constructed play's:
//! Phyrexian Metamorph, Mockingbird, the cube's double-faced creatures) plus
//! Edge of Eternities' Terminal Velocity.
//!
//! Terminal Velocity {4}{R}{R}, Sorcery: "You may put an artifact or creature
//! card from your hand onto the battlefield. That permanent gains haste, 'When
//! this permanent leaves the battlefield, it deals damage equal to its mana
//! value to each creature,' and 'At the beginning of your end step, sacrifice
//! this permanent.'"

use std::sync::OnceLock;

use engine::db::{CardDb, Counter};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

const TV: &str = "Terminal Velocity";
const METAMORPH: &str = "Phyrexian Metamorph";
const MB: &str = "Mockingbird";
const ODDITY: &str = "Ulvenwald Oddity // Ulvenwald Behemoth";
const BEHEMOTH: &str = "Ulvenwald Behemoth";
const CURTAINS: &str = "Concealing Curtains // Revealing Eye";
const EYE: &str = "Revealing Eye";
const RABBIT: &str = "Jacked Rabbit";
const THOR: &str = "Thor, God of Thunder";
const GSZ: &str = "Green Sun's Zenith";
const COUNTERSPELL: &str = "Counterspell";

fn db() -> &'static CardDb {
    static DB: OnceLock<&'static CardDb> = OnceLock::new();
    DB.get_or_init(|| {
        let mut cards = engine::db::load_cube_document().cards;
        cards.extend(engine::db::load_legacy_document().cards);
        cards.extend(engine::db::load_eoe_document().cards.into_iter().filter(|c| c.name == TV));
        let db = CardDb::from_cards(&cards);
        for n in [TV, METAMORPH, MB, ODDITY, CURTAINS, RABBIT, THOR, GSZ, COUNTERSPELL] {
            db.expect_id(n).unwrap_or_else(|e| panic!("{e}"));
        }
        db.leak()
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

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// `p`'s permanent whose name is `name` now (a transformed or copying one).
fn named(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

/// Pay with the first source offered until the payment is over.
fn pay(g: &mut Game) {
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    while matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(|g| at_priority(g) || !matches!(g.kind(), Some(DecisionKind::PayMana) | Some(DecisionKind::ChooseMana)));
    }
}

/// Run the stack down; `answer` answers what is asked (None: the first option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g));
        if g.stack().is_empty() && at_priority(g) {
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

/// Pass, decline attacks and blocks, the first option otherwise, until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress\n{}", log(g));
}

/// P0 casts Terminal Velocity and puts `card` from its hand onto the
/// battlefield; it enters as a copy of `copy` if it may (`None`: as itself).
/// Returns the card.
fn terminal_velocity(g: &mut Game, card: &str, copy: Option<CardIdx>) -> CardIdx {
    let c = g.card(0, card, Zone::Hand);
    let tv = g.card(0, TV, Zone::Hand);
    g.choose(Choice::Cast(tv));
    pay(g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    g.choose(Choice::Card(c));
    resolve_with(g, |g| match g.kind() {
        // What it enters as a copy of (CR 707.5; `PASS` = nothing).
        Some(DecisionKind::ChoosePermanent) if g.has_option(Choice::Pass) => Some(copy.map_or(Choice::Pass, Choice::Card)),
        _ => None,
    });
    assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}", log(g));
    c
}

/// On to P0's end step: Terminal Velocity's granted "sacrifice this
/// permanent" resolves, then its leave trigger.
fn end_step(g: &mut Game) {
    let t = g.turn().number;
    quiet_until(g, |g| g.turn().number == t && g.turn().step == Step::End);
    resolve_with(g, |_| None);
}

/// The departed source's leave trigger was put on the stack in the end step
/// after the sacrifice trigger (damage 0 is no damage event, so the log
/// shows the trigger instead), and the stack has resolved.
fn leave_trigger_resolved(g: &Game) -> bool {
    let n = g.log_lines().iter().filter(|l| l.contains(" End ") && l.contains("trigger of Terminal Velocity goes on the stack")).count();
    n == 2 && g.stack().is_empty()
}

#[test]
fn cr_202_3b_a_departed_copy_of_a_back_face_deals_damage_equal_to_mana_value_0() {
    // P1's Ulvenwald Oddity transformed (Ulvenwald Behemoth 8/8: mana value
    // 4, its front face's, CR 202.3b). Terminal Velocity puts Phyrexian
    // Metamorph onto the battlefield as a copy of it: a copy of a back face,
    // mana value 0 (202.3b). Sacrificed at the end step, it deals damage
    // equal to its mana value as it last existed (608.2h, 113.7a): 0, not
    // the front face's 4 its copiable values alone would give.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(1, &[ODDITY, CURTAINS])
        .transformed(1, ODDITY)
        .hand(0, &[TV, METAMORPH])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let beh = named(&g, 1, BEHEMOTH);
    let cur = g.card(1, CURTAINS, Zone::Battlefield);
    assert_eq!(g.mana_value_now(beh), 4, "a back face: its front face's mana value");
    let m = terminal_velocity(&mut g, METAMORPH, Some(beh));
    assert_eq!(g.name(m), BEHEMOTH);
    assert!(g.is_copy(m));
    assert_eq!(g.mana_value_now(m), 0, "a copy of a back face (CR 202.3b)");
    end_step(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Graveyard, "sacrificed\n{}", log(&g));
    assert!(leave_trigger_resolved(&g), "{}", log(&g));
    assert_eq!(g.obj(beh).damage, 0, "0 damage: the copy's mana value as it last existed\n{}", log(&g));
    assert_eq!(g.obj(cur).damage, 0);
}

#[test]
fn cr_202_3b_a_departed_back_face_not_a_copy_deals_its_front_faces_mana_value() {
    // Terminal Velocity puts Concealing Curtains onto the battlefield; it
    // transforms ({2}{B}) into Revealing Eye (mana value 1: its front face's,
    // CR 202.3b; not a copy). Sacrificed at the end step it deals 1 damage to
    // each creature (608.2h).
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(0, &["Swamp"; 3])
        .battlefield(1, &[RABBIT])
        .hand(0, &[TV, CURTAINS])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let rabbit = g.card(1, RABBIT, Zone::Battlefield);
    let c = terminal_velocity(&mut g, CURTAINS, None);
    let transform = Choice::Ability(c, engine::db::AbilitySlot::Activated(0));
    assert!(g.has_option(transform), "{:?}", g.options_desc());
    g.choose(transform);
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.name(c), EYE, "{}", log(&g));
    assert!(!g.is_copy(c));
    assert_eq!(g.mana_value_now(c), 1);
    end_step(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(rabbit).damage, 1, "its front face's mana value\n{}", log(&g));
}

#[test]
fn cr_608_2h_a_departed_copy_deals_the_copied_mana_value_not_its_cards() {
    // Phyrexian Metamorph ({3}{U/P}, mana value 4) put by Terminal Velocity
    // as a copy of Concealing Curtains (mana value 1): it last existed with
    // mana value 1, so it deals 1 damage to each creature (608.2h), not its
    // card's 4 in the graveyard.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(1, &[CURTAINS, RABBIT])
        .hand(0, &[TV, METAMORPH])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let cur = g.card(1, CURTAINS, Zone::Battlefield);
    let rabbit = g.card(1, RABBIT, Zone::Battlefield);
    let m = terminal_velocity(&mut g, METAMORPH, Some(cur));
    assert_eq!(g.name(m), CURTAINS);
    assert_eq!(g.mana_value_now(m), 1);
    end_step(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.mana_value_now(m), 4, "the card in the graveyard");
    assert_eq!(g.obj(rabbit).damage, 1, "{}", log(&g));
    assert_eq!(g.obj(cur).damage, 1);
}

#[test]
fn cr_202_3b_707_9b_a_departed_mockingbird_copying_a_copy_of_a_back_face_deals_0() {
    // P0's Phyrexian Metamorph (cast) copies P1's transformed Ulvenwald
    // Oddity: a copy of a back face, mana value 0 (202.3b). Terminal Velocity
    // then puts Mockingbird onto the battlefield (not cast: no mana spent, so
    // only a creature with mana value 0 may be copied): it copies the
    // Metamorph, so it is a copy of the back face too (with the Metamorph's
    // "artifact" exception and its own Bird and flying, 707.9b), mana value 0.
    // Sacrificed, it deals 0 damage (608.2h), not the front face's 4.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(0, &["Island"; 4])
        .battlefield(1, &[ODDITY, RABBIT])
        .transformed(1, ODDITY)
        .counters(1, RABBIT, Counter::P1P1, 5)
        .hand(0, &[TV, METAMORPH, MB])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let beh = named(&g, 1, BEHEMOTH);
    let rabbit = g.card(1, RABBIT, Zone::Battlefield);
    let m = g.card(0, METAMORPH, Zone::Hand);
    g.choose(Choice::Cast(m));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    // (Its Phyrexian pip paid with {U}, not life.)
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) if g.has_option(Choice::Pass) => Some(Choice::Card(beh)),
        _ => None,
    });
    assert_eq!(g.name(m), BEHEMOTH, "{}", log(&g));
    assert_eq!(g.mana_value_now(m), 0);
    let b = terminal_velocity(&mut g, MB, Some(m));
    assert_eq!(g.name(b), BEHEMOTH, "{}", log(&g));
    assert!(g.is_copy(b));
    assert_eq!(g.mana_value_now(b), 0, "a copy of a copy of a back face");
    end_step(&mut g);
    assert_eq!(g.obj(b).zone, Zone::Graveyard, "{}", log(&g));
    assert!(leave_trigger_resolved(&g), "{}", log(&g));
    // A 6/7: it would survive the front face's 4, so its 0 is the damage dealt.
    assert_eq!(g.obj(rabbit).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(rabbit).damage, 0, "{}", log(&g));
    assert_eq!(g.obj(beh).damage, 0);
    assert_eq!(g.obj(m).damage, 0);
}

#[test]
fn cr_202_3e_a_departed_copy_of_an_x_creature_counts_x_as_0() {
    // P1's Jacked Rabbit ({X}{1}{W}) with three +1/+1 counters. Phyrexian
    // Metamorph put by Terminal Velocity copies it: mana value 2 (X is 0 off
    // the stack, 202.3e); it enters with no counters (it wasn't cast: X 0,
    // 107.3m), a 1/2. Sacrificed, it deals 2 damage to each creature.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 6])
        .battlefield(1, &[RABBIT, CURTAINS])
        .counters(1, RABBIT, Counter::P1P1, 3)
        .hand(0, &[TV, METAMORPH])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let rabbit = g.card(1, RABBIT, Zone::Battlefield);
    let cur = g.card(1, CURTAINS, Zone::Battlefield);
    let m = terminal_velocity(&mut g, METAMORPH, Some(rabbit));
    assert_eq!(g.name(m), RABBIT);
    assert_eq!(g.counters_on(m, Counter::P1P1), 0);
    assert_eq!(g.mana_value_now(m), 2);
    end_step(&mut g);
    assert_eq!(g.obj(m).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(rabbit).damage, 2, "{}", log(&g));
    assert_eq!(g.obj(cur).damage, 2);
}

#[test]
fn cr_608_2h_202_3e_thor_a_countered_x_spell_deals_its_mana_value_with_x() {
    // Thor: "Whenever you cast a noncreature spell, Thor deals damage equal
    // to that spell's mana value to any target." Green Sun's Zenith cast
    // with X 2 (mana value 3, 202.3e); P1 counters it in response to the
    // trigger: the spell as it last existed on the stack (608.2h), X
    // included: 3 damage.
    let mut g = scen()
        .battlefield(0, &[THOR, "Forest", "Forest", "Forest"])
        .battlefield(1, &["Island", "Island"])
        .hand(0, &[GSZ])
        .hand(1, &[COUNTERSPELL])
        .at(Step::Main1, 0)
        .turn_number(3)
        .start();
    let z = g.card(0, GSZ, Zone::Hand);
    g.choose(Choice::Cast(z));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    g.choose(Choice::Number(2));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(|g| at_priority(g) && g.pending_decision().unwrap().player == 1);
    let cs = g.card(1, COUNTERSPELL, Zone::Hand);
    g.choose(Choice::Cast(cs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(z));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(z).zone, Zone::Graveyard, "countered\n{}", log(&g));
    assert_eq!(g.life(1), 17, "3 damage: X 2 and {{G}}\n{}", log(&g));
}
