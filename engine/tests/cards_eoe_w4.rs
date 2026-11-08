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

// ---- Scout for Survivors ------------------------------------------------------------

/// Scout for Survivors: "Return up to three target creature cards with total
/// mana value 3 or less from your graveyard to the battlefield. Put a +1/+1
/// counter on each of them." The targets are chosen one at a time, each
/// offered only if the total stays within 3 (CR 601.2c: the announced set's
/// restriction); cards in the opponent's graveyard are not targets.
#[test]
fn cr_601_2c_scout_for_survivors_offers_only_targets_within_the_total() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .graveyard(0, &["W4 Elf", "W4 Bear", "W4 Ogre", "W4 Giant"])
        .graveyard(1, &["W4 Elf"])
        .hand(0, &["Scout for Survivors"])
        .at(Step::Main1, 0)
        .start();
    let gy = |g: &Game, p, n| g.card(p, n, Zone::Graveyard);
    let (elf, bear, ogre, giant, theirs) = (gy(&g, 0, "W4 Elf"), gy(&g, 0, "W4 Bear"), gy(&g, 0, "W4 Ogre"), gy(&g, 0, "W4 Giant"), gy(&g, 1, "W4 Elf"));
    let s = g.card(0, "Scout for Survivors", Zone::Hand);
    g.choose(Choice::Cast(s));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    for (c, ok) in [(elf, true), (bear, true), (ogre, true), (giant, false), (theirs, false)] {
        assert_eq!(g.has_option(g.tgt(c)), ok, "{} {:?}", g.name(c), g.options_desc());
    }
    assert!(g.has_option(Choice::Pass), "up to three");
    g.choose(g.tgt(elf));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana));
    assert!(g.has_option(g.tgt(bear)), "1 + 2 = 3");
    assert!(!g.has_option(g.tgt(ogre)), "1 + 3 = 4: over the total");
    g.choose(g.tgt(bear));
    // Nothing is left within the total (1 + 2 = 3): no third target, only
    // "no more" (a single option, which `auto_single` answers in play).
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || g.kind() == Some(DecisionKind::PayMana));
    if g.kind() == Some(DecisionKind::Target) {
        assert_eq!(g.options_desc().len(), 1, "no candidate within the total: {:?}", g.options_desc());
        g.choose(Choice::Pass);
    }
    pay(&mut g);
    settle(&mut g);
    for (c, pt) in [(elf, (2, 2)), (bear, (3, 3))] {
        assert_eq!(g.obj(c).zone, Zone::Battlefield, "{}\n{}", g.name(c), log(&g));
        assert_eq!(g.obj(c).controller, 0);
        assert_eq!(g.pt(c), pt, "a +1/+1 counter on each (CR 122.1)");
    }
    assert_eq!(g.obj(ogre).zone, Zone::Graveyard);
}

/// A mana value 3 card alone fills the total; with no target at all the
/// spell does nothing (up to three, CR 601.2c: zero is allowed).
#[test]
fn scout_for_survivors_one_three_drop_or_nothing() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .graveyard(0, &["W4 Elf", "W4 Ogre"])
        .hand(0, &["Scout for Survivors"])
        .at(Step::Main1, 0)
        .start();
    let ogre = g.card(0, "W4 Ogre", Zone::Graveyard);
    let elf = g.card(0, "W4 Elf", Zone::Graveyard);
    let s = g.card(0, "Scout for Survivors", Zone::Hand);
    g.choose(Choice::Cast(s));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(ogre));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Target) || g.has_option(g.tgt(elf)));
    assert!(!g.has_option(g.tgt(elf)), "3 + 1 = 4");
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(ogre).zone, Zone::Battlefield);
    assert_eq!(g.pt(ogre), (4, 4));
    assert_eq!(g.obj(elf).zone, Zone::Graveyard);
    // No target: castable, does nothing.
    let mut g = scen().battlefield(0, &["Plains", "Plains", "Plains"]).hand(0, &["Scout for Survivors"]).at(Step::Main1, 0).start();
    let s = g.card(0, "Scout for Survivors", Zone::Hand);
    assert!(g.has_option(Choice::Cast(s)), "up to three: zero targets");
    g.choose(Choice::Cast(s));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Target));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(s).zone, Zone::Graveyard);
}

/// The returned creatures enter together, then get their counters: one
/// that left the graveyard before the spell resolves is not returned, the
/// other still is (CR 608.2b).
#[test]
fn cr_608_2b_scout_for_survivors_returns_the_targets_still_there() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains"])
        .graveyard(0, &["W4 Elf", "W4 Bear"])
        .hand(0, &["Scout for Survivors"])
        .battlefield(1, &["Swamp"])
        .hand(1, &["W4 Purge"])
        .at(Step::Main1, 0)
        .start();
    let elf = g.card(0, "W4 Elf", Zone::Graveyard);
    let bear = g.card(0, "W4 Bear", Zone::Graveyard);
    let s = g.card(0, "Scout for Survivors", Zone::Hand);
    g.choose(Choice::Cast(s));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(elf));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Target));
    pay(&mut g);
    // The opponent exiles the Bear in response.
    to_opponent_priority(&mut g);
    let tb = g.tgt(bear);
    cast(&mut g, 1, "W4 Purge", &[tb]);
    settle(&mut g);
    assert_eq!(g.obj(elf).zone, Zone::Battlefield);
    assert_eq!(g.pt(elf), (2, 2));
    assert_eq!(g.obj(bear).zone, Zone::Exile);
}

// ---- Chorale of the Void ------------------------------------------------------------

/// Chorale of the Void: "Enchant creature you control / Whenever enchanted
/// creature attacks, put target creature card from defending player's
/// graveyard onto the battlefield under your control tapped and attacking."
/// (CR 303.4a, 508.4: put onto the battlefield attacking, never declared as
/// an attacker.) Only the defending player's graveyard; the creature stays
/// under your control.
#[test]
fn cr_508_4_chorale_of_the_void_reanimates_from_the_defenders_graveyard_attacking() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp", "W4 Bear"])
        .battlefield(1, &["W4 Elf"])
        .graveyard(0, &["W4 Ogre"])
        .graveyard(1, &["W4 Giant"])
        .hand(0, &["Chorale of the Void"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "W4 Bear");
    let elf = bf(&g, 1, "W4 Elf");
    let giant = g.card(1, "W4 Giant", Zone::Graveyard);
    let ogre = g.card(0, "W4 Ogre", Zone::Graveyard);
    let c = g.card(0, "Chorale of the Void", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)), "a creature you control");
    assert!(!g.has_option(g.tgt(elf)), "enchant creature you control");
    g.choose(g.tgt(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(c).zone, Zone::Battlefield);
    attack_with(&mut g, 0, &["W4 Bear"]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(giant)), "the defending player's graveyard");
    assert!(!g.has_option(g.tgt(ogre)), "not your own graveyard");
    g.choose(g.tgt(giant));
    settle(&mut g);
    assert_eq!(g.obj(giant).zone, Zone::Battlefield, "{}", log(&g));
    assert_eq!(g.obj(giant).controller, 0, "under your control");
    assert!(g.obj(giant).tapped && g.obj(giant).attacking, "tapped and attacking");
    quiet_until(&mut g, |g| g.turn().step == Step::End);
    assert_eq!(g.life(1), 13, "2 + 5 combat damage (the Elf didn't block)\n{}", log(&g));
    next_main(&mut g, 1);
    assert_eq!(g.obj(giant).controller, 0, "for good");
}

/// "Void — At the beginning of your end step, sacrifice this Aura unless a
/// nonland permanent left the battlefield this turn or a spell was warped
/// this turn." (CR 603.4.) Without void it is sacrificed; with void it stays.
#[test]
fn cr_603_4_chorale_of_the_void_is_sacrificed_without_void() {
    for void in [false, true] {
        let mut g = scen()
            .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp", "Mountain", "W4 Bear"])
            .battlefield(1, &["W4 Elf"])
            .hand(0, &["Chorale of the Void", "W4 Bolt"])
            .at(Step::Main1, 0)
            .start();
        let bear = bf(&g, 0, "W4 Bear");
        let tb = g.tgt(bear);
        let c = cast(&mut g, 0, "Chorale of the Void", &[tb]);
        settle(&mut g);
        if void {
            let te = g.tgt(bf(&g, 1, "W4 Elf"));
            cast(&mut g, 0, "W4 Bolt", &[te]);
            settle(&mut g);
        }
        quiet_until(&mut g, |g| g.turn().active == 1);
        assert_eq!(g.obj(c).zone == Zone::Battlefield, void, "void {void}\n{}", log(&g));
    }
}

// ---- Blade of the Swarm -------------------------------------------------------------

/// Cast Blade of the Swarm from P0's hand and resolve it, stopping at its
/// enters trigger's mode choice.
fn blade_enters(g: &mut Game) -> CardIdx {
    let b = cast(g, 0, "Blade of the Swarm", &[]);
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode) || (at_priority(g) && g.stack().is_empty()));
    while g.kind() != Some(DecisionKind::ChooseMode) {
        assert!(!g.stack().is_empty(), "the trigger's modes are chosen\n{}", log(g));
        g.pass();
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode) || at_priority(g));
    }
    b
}

/// Blade of the Swarm: "When this creature enters, choose one — Put two
/// +1/+1 counters on this creature." (CR 700.2.) With no exiled card with
/// warp, the second mode can't be chosen (700.2a, 115.1: no legal target).
#[test]
fn cr_700_2_blade_of_the_swarm_puts_two_counters() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp"])
        .exile(1, &["W4 Bear"])
        .hand(0, &["Blade of the Swarm"])
        .at(Step::Main1, 0)
        .start();
    let b = blade_enters(&mut g);
    assert_eq!(g.options_desc().len(), 1, "only the counters mode: {:?}", g.options_desc());
    let c = g.pending_decision().unwrap().options[0].choice;
    g.choose(c);
    settle(&mut g);
    assert_eq!(g.pt(b), (5, 3), "3/1 with two +1/+1 counters\n{}", log(&g));
}

/// "Put target exiled card with warp on the bottom of its owner's library."
/// An exiled card is a legal target (CR 115.1; its option is an `EXILE`
/// id, the k-th exiled card offered); only one with warp (702.185a),
/// whoever owns it; it goes to the bottom of its owner's library (401.4),
/// known to both players there.
#[test]
fn cr_115_1_blade_of_the_swarm_targets_an_exiled_card_with_warp() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp", "Swamp", "Swamp"])
        .exile(1, &["W4 Bear", "Codecracker Hound"])
        .exile(0, &["Germinating Wurm"])
        .hand(0, &["Blade of the Swarm"])
        .at(Step::Main1, 0)
        .start();
    let hound = g.card(1, "Codecracker Hound", Zone::Exile);
    let wurm = g.card(0, "Germinating Wurm", Zone::Exile);
    let bear = g.card(1, "W4 Bear", Zone::Exile);
    let b = blade_enters(&mut g);
    assert_eq!(g.options_desc().len(), 2, "both modes: {:?}", g.options_desc());
    let second = g.pending_decision().unwrap().options[1].choice;
    g.choose(second);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", log(&g));
    assert!(g.has_option(g.tgt(hound)) && g.has_option(g.tgt(wurm)), "exiled cards with warp, either owner");
    assert!(!g.has_option(g.tgt(bear)), "not a card without warp");
    let ids: Vec<u16> = g.pending_decision().unwrap().options.iter().map(|o| o.action).collect();
    assert!(ids.iter().all(|&a| (engine::decision::act::EXILE..engine::decision::act::EXILE + 32).contains(&a)), "EXILE ids: {ids:?}");
    g.choose(g.tgt(hound));
    settle(&mut g);
    assert_eq!(g.obj(hound).zone, Zone::Library, "{}", log(&g));
    assert_eq!(g.player(1).library.first(), Some(&hound), "the bottom of its owner's library");
    assert!(g.knows(0, hound) && g.knows(1, hound), "public before: known to both there");
    assert_eq!(g.pt(b), (3, 1), "no counters");
    assert_eq!(g.obj(wurm).zone, Zone::Exile);
}

// ---- Close Encounter ----------------------------------------------------------------

/// Close Encounter: "As an additional cost to cast this spell, choose a
/// creature you control or a warped creature card you own in exile. Close
/// Encounter deals damage equal to the power of the chosen creature or card
/// to target creature." With creatures only: one `ChoosePermanent` among
/// yours (CR 601.2h; not a target: 115.1); the damage is its power.
#[test]
fn cr_601_2h_close_encounter_chooses_a_creature_you_control() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "W4 Giant", "W4 Elf"])
        .battlefield(1, &["W4 Wurm"])
        .hand(0, &["Close Encounter"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 0, "W4 Giant");
    let elf = bf(&g, 0, "W4 Elf");
    let wurm = bf(&g, 1, "W4 Wurm");
    let c = g.card(0, "Close Encounter", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(wurm));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent) || g.kind() == Some(DecisionKind::PayMana));
    assert_eq!(g.kind(), Some(DecisionKind::ChoosePermanent), "{}", log(&g));
    assert!(g.has_option(Choice::Card(giant)) && g.has_option(Choice::Card(elf)));
    assert!(!g.has_option(Choice::Card(wurm)), "a creature you control");
    g.choose(Choice::Card(giant));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(wurm).damage, 5, "the Giant's power\n{}", log(&g));
}

/// Without a creature you control or a warped creature card in exile the
/// additional cost can't be paid: it can't be cast (CR 601.2h).
#[test]
fn close_encounter_needs_something_to_choose() {
    let g = scen().battlefield(0, &["Forest", "Forest"]).battlefield(1, &["W4 Wurm"]).hand(0, &["Close Encounter"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Close Encounter", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)));
    // An exiled creature card that wasn't warped doesn't count.
    let g = scen()
        .battlefield(0, &["Forest", "Forest"])
        .battlefield(1, &["W4 Wurm"])
        .exile(0, &["Germinating Wurm"])
        .hand(0, &["Close Encounter"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Close Encounter", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "exiled, not warped");
}

/// A warped creature card you own in exile (CR 702.185a: exiled by warp's
/// delayed ability) can be chosen (`ChooseExiled`; `PASS` = a creature you
/// control instead); its power is the card's.
#[test]
fn cr_702_185a_close_encounter_chooses_a_warped_card_in_exile() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "W4 Elf"])
        .battlefield(1, &["W4 Wurm"])
        .hand(0, &["Germinating Wurm", "Close Encounter"])
        .at(Step::Main1, 0)
        .start();
    let gw = g.card(0, "Germinating Wurm", Zone::Hand);
    g.choose(Choice::Cast(gw));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    g.choose(Choice::Number(1));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(gw).zone, Zone::Battlefield, "warped\n{}", log(&g));
    next_main(&mut g, 0);
    assert_eq!(g.obj(gw).zone, Zone::Exile, "exiled at the end step\n{}", log(&g));
    let wurm = bf(&g, 1, "W4 Wurm");
    let elf = bf(&g, 0, "W4 Elf");
    let c = g.card(0, "Close Encounter", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(wurm));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseExiled) || g.kind() == Some(DecisionKind::PayMana));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseExiled), "{}", log(&g));
    assert!(g.has_option(Choice::Pass), "a creature you control instead");
    assert!(g.has_option(Choice::Card(g.card(0, "Germinating Wurm", Zone::Exile))));
    let gw = g.card(0, "Germinating Wurm", Zone::Exile);
    g.choose(Choice::Card(gw));
    assert_ne!(g.kind(), Some(DecisionKind::ChoosePermanent), "one choice");
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(wurm).damage, 5, "the warped card's power\n{}", log(&g));
    assert_eq!(g.obj(gw).zone, Zone::Exile, "only chosen: still in exile");
    assert_eq!(g.obj(elf).zone, Zone::Battlefield);
}

/// The chosen creature leaves the battlefield before Close Encounter
/// resolves: its power as it last existed (CR 608.2h), its pump included.
#[test]
fn cr_608_2h_close_encounter_uses_the_chosen_creatures_last_known_power() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "W4 Bear"])
        .battlefield(1, &["W4 Wurm", "Mountain"])
        .hand(0, &["Close Encounter", "W4 Growth"])
        .hand(1, &["W4 Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 0, "W4 Bear");
    let wurm = bf(&g, 1, "W4 Wurm");
    let tb = g.tgt(bear);
    cast(&mut g, 0, "W4 Growth", &[tb]);
    settle(&mut g);
    let c = g.card(0, "Close Encounter", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(g.tgt(wurm));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChoosePermanent));
    g.choose(Choice::Card(bear));
    pay(&mut g);
    to_opponent_priority(&mut g);
    cast(&mut g, 1, "W4 Bolt", &[tb]);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.obj(wurm).damage, 5, "2 + 3 as it last existed\n{}", log(&g));
}

// ---- The Dominion Bracelet ----------------------------------------------------------

/// The Dominion Bracelet on P0's W4 Wurm (6/6) with nine Islands: equipped
/// (equip {1}), at P0's main phase with the stack empty.
fn bracelet_on_wurm(p1_hand: &[&str]) -> (Game, CardIdx, CardIdx) {
    let mut g = scen()
        .battlefield(0, &["The Dominion Bracelet", "W4 Wurm"])
        .battlefield(0, &["Island"; 9])
        .battlefield(1, &["Mountain", "W4 Bear"])
        .hand(1, p1_hand)
        .at(Step::Main1, 0)
        .start();
    let br = bf(&g, 0, "The Dominion Bracelet");
    let wurm = bf(&g, 0, "W4 Wurm");
    let tw = g.tgt(wurm);
    activate(&mut g, br, 0, &[tw]);
    settle(&mut g);
    assert_eq!(g.obj(br).attached.map(|r| r.idx), Some(wurm), "equipped\n{}", log(&g));
    (g, br, wurm)
}

/// "Equipped creature gets +1/+1 and has '{15}, Exile The Dominion Bracelet:
/// You control target opponent during their next turn. This ability costs
/// {X} less to activate, where X is this creature's power. Activate only as
/// a sorcery.'" The creature has the ability (CR 301.5, 613.1f); its cost is
/// {15} less its power as the cost is determined (7: {8}, CR 601.2f), and
/// the Bracelet is exiled as part of the cost (601.2h).
#[test]
#[ignore = "The Dominion Bracelet is blocked on the encoding (ENGINE.md 99.14); passed at 84932a5: run with --ignored once unblocked"]
fn cr_601_2f_the_dominion_bracelet_grants_the_ability_costing_15_less_power() {
    let (mut g, br, wurm) = bracelet_on_wurm(&[]);
    assert_eq!(g.pt(wurm), (7, 7), "+1/+1");
    let a = act(wurm, 0);
    assert!(g.has_option(a), "the equipped creature has the ability: {:?}", g.options_desc());
    assert_eq!(untapped(&g, 0), 8);
    g.choose(a);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(player(1)) && !g.has_option(player(0)), "target opponent");
    g.choose(player(1));
    pay(&mut g);
    assert_eq!(untapped(&g, 0), 0, "{{15}} less 7: {{8}}\n{}", log(&g));
    assert_eq!(g.obj(br).zone, Zone::Exile, "exiled as a cost");
    assert_eq!(g.pt(wurm), (6, 6));
    settle(&mut g);
    assert_eq!(g.next_turn_controller(1), Some(0), "{}", log(&g));
    assert_eq!(g.turn_controller(), None, "not this turn");
}

/// Not with less than the reduced cost, not on another creature, and only
/// as a sorcery (CR 602.5d).
#[test]
#[ignore = "The Dominion Bracelet is blocked on the encoding (ENGINE.md 99.14); passed at 84932a5: run with --ignored once unblocked"]
fn cr_602_5d_the_dominion_bracelet_ability_only_as_a_sorcery() {
    let (mut g, _br, wurm) = bracelet_on_wurm(&[]);
    to_opponent_priority(&mut g);
    assert!(!g.has_option(act(wurm, 0)), "not with something on the stack or in another's turn");
    let g = scen().battlefield(0, &["The Dominion Bracelet", "W4 Wurm"]).battlefield(0, &["Island"; 9]).at(Step::Main1, 0).start();
    let wurm = bf(&g, 0, "W4 Wurm");
    assert!(!g.has_option(act(wurm, 0)), "not equipped: no ability");
}

/// "You control target opponent during their next turn" (CR 722.1, 722.4):
/// in that turn every decision of theirs is made by P0 (`to_move`), with
/// their resources (722.4a: the ids are theirs: their hand, their mana);
/// P0 sees what they see (722.3): their hand, and the card they draw. The
/// turn after, each player decides for themselves again (722.1).
#[test]
#[ignore = "The Dominion Bracelet is blocked on the encoding (ENGINE.md 99.14); passed at 84932a5: run with --ignored once unblocked"]
fn cr_722_4_the_controller_makes_the_controlled_players_decisions() {
    let (mut g, _br, wurm) = bracelet_on_wurm(&["W4 Bolt", "W4 Giant"]);
    let p1_hand: Vec<CardIdx> = g.player(1).hand.clone();
    assert!(p1_hand.iter().all(|&c| !g.knows(0, c)), "hidden before");
    g.choose(act(wurm, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    settle(&mut g);
    // To P1's turn: its first decision is decided by P0.
    quiet_until(&mut g, |g| g.turn().active == 1 && at_priority(g));
    assert_eq!(g.turn_controller(), Some(0));
    assert_eq!(g.pending_decision().unwrap().player, 1, "P1's decision");
    assert_eq!(g.to_move(), Some(0), "made by P0 (CR 722.4)");
    for &c in &g.player(1).hand {
        assert!(g.knows(0, c), "P0 sees P1's hand (CR 722.3): {}", g.name(c));
    }
    // P1's main phase: P0 casts P1's Bolt at P1's own Bear.
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.to_move(), Some(0));
    for &c in &g.player(1).hand {
        assert!(g.knows(0, c), "and what P1 drew: {}", g.name(c));
    }
    let bear = bf(&g, 1, "W4 Bear");
    let tb = g.tgt(bear);
    cast(&mut g, 1, "W4 Bolt", &[tb]);
    assert_eq!(g.to_move(), Some(0), "P0 still decides (P1's priority)");
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard, "{}", log(&g));
    // P0's own decisions in P1's turn stay P0's (CR 722.7); the next turn is
    // P0's own, and P1's turn after that is P1's.
    quiet_until(&mut g, |g| g.turn().active == 0 && at_priority(g));
    assert_eq!(g.turn_controller(), None);
    assert_eq!(g.to_move(), Some(0));
    quiet_until(&mut g, |g| g.turn().active == 1 && at_priority(g));
    assert_eq!(g.turn_controller(), None, "only the next turn");
    assert_eq!(g.to_move(), Some(1));
}

/// Hidden information in a controlled turn: `determinize` for the decider
/// (P0) keeps its view and its actions (P1's hand is known to it, CR 722.3,
/// so never resampled); for P1 too. P1's library stays hidden from P0.
#[test]
#[ignore = "The Dominion Bracelet is blocked on the encoding (ENGINE.md 99.14); passed at 84932a5: run with --ignored once unblocked"]
fn cr_722_3_a_controlled_turn_survives_determinize() {
    let (mut g, _br, wurm) = bracelet_on_wurm(&["W4 Bolt", "W4 Giant", "W4 Elf"]);
    g.choose(act(wurm, 0));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(player(1));
    pay(&mut g);
    settle(&mut g);
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Main1 && at_priority(g));
    assert_eq!(g.to_move(), Some(0));
    assert!(g.player(1).library.iter().all(|&c| !g.knows(0, c)), "P1's library stays hidden");
    let mut rng = engine::rng::Rng::new(722);
    for viewer in [0u8, 1] {
        for _ in 0..4 {
            let mut d = g.clone();
            d.determinize(viewer, &mut rng);
            assert_eq!(d.to_move(), Some(0), "viewer P{viewer}");
            assert_eq!(g.view_of(viewer), d.view_of(viewer), "viewer P{viewer}: the view");
            let (mut a, mut b) = (Vec::new(), Vec::new());
            g.legal_actions(&mut a);
            d.legal_actions(&mut b);
            assert_eq!(a, b, "viewer P{viewer}: the decider's actions");
            if viewer == 0 {
                let names = |g: &Game| {
                    let mut v: Vec<&str> = g.player(1).hand.iter().map(|&c| g.name(c)).collect();
                    v.sort();
                    v
                };
                assert_eq!(names(&g), names(&d), "P1's hand, known to P0, is never resampled");
            }
        }
    }
}
