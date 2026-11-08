//! W4 of the Edge of Eternities port (ENGINE.md 99.14): exile, graveyard,
//! copies and control. Each card against its Oracle text; test names cite
//! the CR.

mod eoe_w4_kit;

use engine::db::types;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Zone};
use eoe_w4_kit::*;

fn tokens_named(g: &Game, p: engine::PlayerId, name: &str) -> Vec<CardIdx> {
    g.permanents_of(p).filter(|&c| g.name(c) == name && g.is_token(c)).collect()
}

/// Activate `c`'s ability `slot`, answering targets and costs with `answers`
/// in order (each when it is offered), then pay with the first source.
fn activate(g: &mut Game, c: CardIdx, slot: u8, answers: &[Choice]) {
    assert!(g.has_option(act(c, slot)), "ability {slot} offered: {:?}\n{}", g.options_desc(), log(g));
    g.choose(act(c, slot));
    for &a in answers {
        g.drive_until(|g| g.has_option(a) || at_priority(g));
        assert!(g.has_option(a), "{a:?} offered: {:?}\n{}", g.options_desc(), log(g));
        g.choose(a);
    }
    pay(g);
}

// ---- Susur Secundi, Void Altar ----------------------------------------------------

/// Susur Secundi, Void Altar: "12+ | {1}{B}, {T}, Pay 2 life, Sacrifice a
/// creature: Draw cards equal to the sacrificed creature's power. Activate
/// only as a sorcery." Not with 11 charge counters (CR 702.184a: the "12+"
/// ability), and not while something is on the stack (602.5d).
#[test]
fn cr_702_184a_susur_secundi_needs_twelve_counters() {
    let g = scen()
        .battlefield(0, &["Susur Secundi, Void Altar", "Swamp", "Swamp", "W4 Giant"])
        .counters(0, "Susur Secundi, Void Altar", charge(), 11)
        .at(Step::Main1, 0)
        .start();
    assert!(!g.has_option(act(bf(&g, 0, "Susur Secundi, Void Altar"), 1)), "11 counters");
    let g = scen()
        .battlefield(0, &["Susur Secundi, Void Altar", "Swamp", "Swamp", "W4 Giant"])
        .counters(0, "Susur Secundi, Void Altar", charge(), 12)
        .at(Step::Main1, 0)
        .start();
    assert!(g.has_option(act(bf(&g, 0, "Susur Secundi, Void Altar"), 1)), "12 counters, main phase, empty stack");
    let g = scen()
        .battlefield(0, &["Susur Secundi, Void Altar", "Swamp", "Swamp", "W4 Giant"])
        .counters(0, "Susur Secundi, Void Altar", charge(), 12)
        .at(Step::Main1, 1)
        .start();
    assert!(!g.has_option(act(bf(&g, 0, "Susur Secundi, Void Altar"), 1)), "only as a sorcery (the opponent's turn)");
}

/// The draw is the sacrificed creature's power as it last existed on the
/// battlefield (CR 608.2h), its pump included; the costs are paid (2 life,
/// the sacrifice, the tap: CR 602.2b, 118.3).
#[test]
fn cr_608_2h_susur_secundi_draws_the_sacrificed_creatures_last_known_power() {
    let mut g = scen()
        .battlefield(0, &["Susur Secundi, Void Altar", "Swamp", "Swamp", "Forest", "W4 Bear", "W4 Giant"])
        .counters(0, "Susur Secundi, Void Altar", charge(), 12)
        .hand(0, &["W4 Growth"])
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, "Susur Secundi, Void Altar");
    let bear = bf(&g, 0, "W4 Bear");
    let giant = bf(&g, 0, "W4 Giant");
    let tb = g.tgt(bear);
    cast(&mut g, 0, "W4 Growth", &[tb]);
    settle(&mut g);
    assert_eq!(g.pt(bear), (5, 5));
    let hand = g.player(0).hand.len();
    activate(&mut g, s, 1, &[Choice::Card(bear)]);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "sacrificed as a cost\n{}", log(&g));
    assert_eq!(g.obj(giant).zone, Zone::Battlefield);
    assert_eq!(g.life(0), 18, "2 life paid");
    assert!(g.obj(s).tapped);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 5, "the Bear's power as it last existed: 2 + 3\n{}", log(&g));
}

/// A 0-power creature sacrificed draws nothing; the ability still resolves.
#[test]
fn susur_secundi_sacrificing_a_zero_power_creature_draws_nothing() {
    let mut g = scen()
        .battlefield(0, &["Susur Secundi, Void Altar", "Swamp", "Swamp", "W4 Elf"])
        .counters(0, "Susur Secundi, Void Altar", charge(), 12)
        .at(Step::Main1, 0)
        .start();
    let s = bf(&g, 0, "Susur Secundi, Void Altar");
    let elf = bf(&g, 0, "W4 Elf");
    let hand = g.player(0).hand.len();
    activate(&mut g, s, 1, &[Choice::Card(elf)]);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 1, "the Elf's power: 1");
}

// ---- Adagia, Windswept Bastion ----------------------------------------------------

/// Adagia, Windswept Bastion: "12+ | {3}{W}, {T}: Create a token that's a copy
/// of target artifact or enchantment you control, except it's legendary.
/// Activate only as a sorcery." The token is a copy (CR 707.2) with the
/// legendary supertype as a copiable exception (707.9b); the original is
/// unchanged. Only an artifact or enchantment you control is a target.
#[test]
fn cr_707_9b_adagia_copies_an_artifact_except_its_legendary() {
    let mut g = scen()
        .battlefield(0, &["Adagia, Windswept Bastion", "Plains", "Plains", "Plains", "Plains", "W4 Trinket", "W4 Bear"])
        .battlefield(1, &["W4 Totem"])
        .counters(0, "Adagia, Windswept Bastion", charge(), 12)
        .at(Step::Main1, 0)
        .start();
    let a = bf(&g, 0, "Adagia, Windswept Bastion");
    let trinket = bf(&g, 0, "W4 Trinket");
    let bear = bf(&g, 0, "W4 Bear");
    let theirs = bf(&g, 1, "W4 Totem");
    g.choose(act(a, 1));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(trinket)), "an artifact you control");
    assert!(!g.has_option(g.tgt(bear)), "not a creature");
    assert!(!g.has_option(g.tgt(theirs)), "not the opponent's enchantment");
    g.choose(g.tgt(trinket));
    pay(&mut g);
    settle(&mut g);
    let toks = tokens_named(&g, 0, "W4 Trinket");
    assert_eq!(toks.len(), 1, "{}", log(&g));
    let t = toks[0];
    assert!(g.is_legendary(t), "except it's legendary");
    assert!(g.types_of(t) & types::ARTIFACT != 0);
    assert!(!g.is_legendary(trinket), "the original isn't");
    assert!(g.obj(a).tapped);
}

/// Two legendary token copies with the same name: the legend rule (CR
/// 704.5j) makes their controller keep one; the non-legendary original is
/// not involved.
#[test]
fn cr_704_5j_two_adagia_copies_are_subject_to_the_legend_rule() {
    let mut g = scen()
        .battlefield(0, &["Adagia, Windswept Bastion", "Plains", "Plains", "Plains", "Plains", "W4 Totem"])
        .counters(0, "Adagia, Windswept Bastion", charge(), 12)
        .at(Step::Main1, 0)
        .start();
    let a = bf(&g, 0, "Adagia, Windswept Bastion");
    let totem = bf(&g, 0, "W4 Totem");
    let tt = g.tgt(totem);
    activate(&mut g, a, 1, &[tt]);
    settle(&mut g);
    assert_eq!(tokens_named(&g, 0, "W4 Totem").len(), 1);
    // Next turn, a second copy of the original.
    next_main(&mut g, 0);
    let tt = g.tgt(totem);
    activate(&mut g, a, 1, &[tt]);
    settle(&mut g);
    assert_eq!(tokens_named(&g, 0, "W4 Totem").len(), 1, "one legendary copy kept\n{}", log(&g));
    assert_eq!(g.obj(totem).zone, Zone::Battlefield, "the original isn't legendary");
}

// ---- Alpharael, Stonechosen ---------------------------------------------------------

/// Alpharael, Stonechosen: "Ward—Discard a card at random." The opponent's
/// spell targeting it is countered unless they discard a card at random (CR
/// 702.21a, 701.9b): asked as a yes/no; paid, one of their two other cards
/// is discarded and the spell resolves.
#[test]
fn cr_702_21a_alpharael_ward_paid_by_a_random_discard() {
    let mut g = scen()
        .battlefield(0, &["Alpharael, Stonechosen"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["W4 Bolt", "W4 Bear", "W4 Giant"])
        .at(Step::Main1, 1)
        .start();
    let a = bf(&g, 0, "Alpharael, Stonechosen");
    let ta = g.tgt(a);
    cast(&mut g, 1, "W4 Bolt", &[ta]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo) || (at_priority(g) && g.stack().len() == 2));
    while g.kind() != Some(DecisionKind::YesNo) {
        g.pass();
        g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo) || at_priority(g));
    }
    let d = g.pending_decision().unwrap();
    assert_eq!(d.player, 1, "the spell's controller pays");
    assert!(g.has_option(Choice::Yes) && g.has_option(Choice::Pass));
    g.choose(Choice::Yes);
    assert_eq!(g.player(1).hand.len(), 1, "one card discarded at random\n{}", log(&g));
    assert_eq!(g.player(1).graveyard.len(), 1);
    settle(&mut g);
    assert_eq!(g.obj(a).zone, Zone::Graveyard, "the Bolt resolved\n{}", log(&g));
}

/// Not paid (declined, or no card to discard: only "no" is offered), the
/// spell is countered.
#[test]
fn cr_702_21a_alpharael_ward_counters_when_not_paid() {
    for cards in [&["W4 Bolt", "W4 Bear"][..], &["W4 Bolt"][..]] {
        let mut g = scen().battlefield(0, &["Alpharael, Stonechosen"]).battlefield(1, &["Mountain"]).hand(1, cards).at(Step::Main1, 1).start();
        let a = bf(&g, 0, "Alpharael, Stonechosen");
        let ta = g.tgt(a);
        let bolt = cast(&mut g, 1, "W4 Bolt", &[ta]);
        g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo) || at_priority(g));
        while g.kind() != Some(DecisionKind::YesNo) {
            g.pass();
            g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo) || at_priority(g));
        }
        assert_eq!(g.has_option(Choice::Yes), cards.len() > 1, "yes only with a card in hand");
        g.choose(Choice::Pass);
        settle(&mut g);
        assert_eq!(g.obj(a).zone, Zone::Battlefield, "countered");
        assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
        assert_eq!(g.player(1).hand.len(), cards.len() - 1);
    }
}

/// "Void — Whenever Alpharael attacks, if a nonland permanent left the
/// battlefield this turn or a spell was warped this turn, defending player
/// loses half their life, rounded up." (CR 603.4: an intervening "if";
/// 107.1a: rounded up.) Without void, nothing.
#[test]
fn cr_603_4_alpharael_halves_the_defenders_life_with_void() {
    // No void: no trigger.
    let mut g = scen().battlefield(0, &["Alpharael, Stonechosen"]).life(1, 15).at(Step::Main1, 0).start();
    attack_with(&mut g, 0, &["Alpharael, Stonechosen"]);
    quiet_until(&mut g, |g| g.turn().step == Step::End);
    assert_eq!(g.life(1), 12, "only the combat damage\n{}", log(&g));
    // Void (the opponent's creature died this turn): 15 -> 7, then 3 damage.
    let mut g = scen()
        .battlefield(0, &["Alpharael, Stonechosen", "Mountain"])
        .battlefield(1, &["W4 Bear"])
        .hand(0, &["W4 Bolt"])
        .life(1, 15)
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "W4 Bear");
    let tb = g.tgt(bear);
    cast(&mut g, 0, "W4 Bolt", &[tb]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    attack_with(&mut g, 0, &["Alpharael, Stonechosen"]);
    settle(&mut g);
    assert_eq!(g.life(1), 7, "15 loses 8 (half, rounded up)\n{}", log(&g));
    quiet_until(&mut g, |g| g.turn().step == Step::End);
    assert_eq!(g.life(1), 4);
}
