//! M7c, the single-card tail part 2 (ENGINE.md 29): the cards that needed
//! two features each. Test names cite the CR.

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| {
        s.auto_single = false;
        s.log = true;
    }).library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn subtype(name: &str) -> u16 {
    db().subtype_id(name).unwrap()
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

/// Run the stack down; `answer` answers what is asked (None: first option).
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

fn settle(g: &mut Game) {
    resolve_with(g, |_| None);
}

/// Pass, decline attacks and blocks, first option otherwise, until `stop`.
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

/// Cast `name` from P0's hand and pay with the first sources.
fn cast(g: &mut Game, name: &str) -> CardIdx {
    let c = g.card(0, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    pay(g);
    c
}

// ---- Portal to Phyrexia ----------------------------------------------------

/// "When this artifact enters, each opponent sacrifices three creatures of
/// their choice" (CR 701.21a: chosen, then sacrificed at once); "at the
/// beginning of your upkeep, put target creature card from a graveyard onto
/// the battlefield under your control. It's a Phyrexian in addition to its
/// other types."
#[test]
fn portal_to_phyrexia_three_sacrificed_at_once_cr701_21a_then_reanimates_a_phyrexian() {
    let mut g = scen()
        .battlefield(0, &["Island"; 9])
        .hand(0, &["Portal to Phyrexia"])
        .battlefield(1, &["Test Bear", "Test Flyer", "Test Giant", "Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Portal to Phyrexia");
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    assert_eq!(g.pending_decision().unwrap().player, 1, "the opponent chooses");
    // Increasing slot order, leaving enough: the first pick can't be the
    // last creature.
    let giant = bf(&g, 1, "Test Giant");
    let reacher = bf(&g, 1, "Test Reacher");
    assert!(!g.has_option(Choice::Card(reacher)));
    g.choose(Choice::Card(bf(&g, 1, "Test Bear")));
    g.choose(Choice::Card(bf(&g, 1, "Test Flyer")));
    assert!(g.has_option(Choice::Card(giant)) && g.has_option(Choice::Card(reacher)));
    g.choose(Choice::Card(reacher));
    settle(&mut g);
    assert_eq!(g.creatures_of(1).collect::<Vec<_>>(), vec![giant], "{}", log(&g));
    assert_eq!(g.graveyard_cards(1).count(), 3);
    // P0's next upkeep: the Flyer from P1's graveyard.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.kind() == Some(DecisionKind::Target));
    let flyer = g.card(1, "Test Flyer", Zone::Graveyard);
    g.choose(g.tgt(flyer));
    settle(&mut g);
    assert_eq!(g.obj(flyer).zone, Zone::Battlefield);
    assert_eq!(g.obj(flyer).controller, 0, "under your control");
    assert!(g.has_subtype(flyer, subtype("Phyrexian")) && g.is_creature(flyer), "a Phyrexian in addition");
}

/// Fewer creatures than three: all of them, no decision.
#[test]
fn portal_to_phyrexia_with_two_creatures_sacrifices_both_cr701_21a() {
    let mut g = scen()
        .battlefield(0, &["Island"; 9])
        .hand(0, &["Portal to Phyrexia"])
        .battlefield(1, &["Test Bear", "Test Flyer", "Forest"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Portal to Phyrexia");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.creatures_of(1).count(), 0);
    assert_eq!(g.graveyard_cards(1).count(), 2);
}

// ---- The Mind Stone ----------------------------------------------------------

/// "{5}{W}, {T}: Harness The Mind Stone. (Once harnessed, its infinity
/// ability is active.)" "Infinity - At the beginning of your end step,
/// exile up to one other target nonland permanent you control, then return
/// that card to the battlefield under its owner's control."
#[test]
fn the_mind_stone_end_step_flicker_only_once_harnessed() {
    let mut g = scen()
        .battlefield(0, &["The Mind Stone", "Test Bear", "Plains", "Plains", "Plains", "Plains", "Plains", "Plains"])
        .at(Step::Main1, 0)
        .start();
    let stone = bf(&g, 0, "The Mind Stone");
    let bear = bf(&g, 0, "Test Bear");
    // Not harnessed: no trigger at the end step.
    quiet_until(&mut g, |g| g.turn().step == Step::Cleanup || g.turn().number > 1);
    assert_eq!(g.obj_ref(bear), g.obj_ref(bear));
    assert!(!log(&g).contains("Bear Battlefield -> Exile"), "{}", log(&g));
    // Next own turn: harness, then the end step flickers the Bear.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t && g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(act(stone, 1)));
    g.choose(act(stone, 1));
    pay(&mut g);
    settle(&mut g);
    let before = g.obj_ref(bear);
    quiet_until(&mut g, |g| g.kind() == Some(DecisionKind::Target));
    assert!(!g.has_option(g.tgt(stone)), "another permanent");
    g.choose(g.tgt(bear));
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert_ne!(g.obj_ref(bear), before, "a new object (CR 400.7)");
}

// ---- Lurrus of the Dream-Den -----------------------------------------------

/// "Once during each of your turns, you may cast a permanent spell with mana
/// value 2 or less from your graveyard" (CR 601.2a): not a land, not more,
/// once.
#[test]
fn lurrus_casts_one_permanent_of_mana_value_two_or_less_from_the_graveyard_each_turn() {
    let mut g = scen()
        .battlefield(0, &["Lurrus of the Dream-Den", "Forest", "Forest", "Forest", "Forest", "Forest"])
        .graveyard(0, &["Test Bear", "Test Reacher", "Test Giant", "Forest", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    let reacher = g.card(0, "Test Reacher", Zone::Graveyard);
    let giant = g.card(0, "Test Giant", Zone::Graveyard);
    let land = g.card(0, "Forest", Zone::Graveyard);
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    assert!(g.has_option(Choice::Cast(bear)) && g.has_option(Choice::Cast(reacher)));
    assert!(!g.has_option(Choice::Cast(giant)), "mana value 5");
    assert!(!g.has_option(Choice::Play(land)), "a permanent spell, not a land");
    assert!(!g.has_option(Choice::Cast(bolt)), "a permanent spell");
    g.choose(Choice::Cast(bear));
    pay(&mut g);
    settle(&mut g);
    assert_eq!(g.obj(bear).zone, Zone::Battlefield);
    assert!(!g.has_option(Choice::Cast(reacher)), "once each turn");
    // The next own turn: again.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(Choice::Cast(reacher)));
}

// ---- Zirda, the Dawnwaker --------------------------------------------------

/// "Abilities you activate that aren't mana abilities cost {2} less to
/// activate. This effect can't reduce the mana in that cost to less than one
/// mana." (CR 602.2b, 601.2f.)
#[test]
fn zirda_reduces_activation_costs_by_two_leaving_one_mana_cr601_2f() {
    // The Mind Stone's {5}{W} harness: {3}{W} with Zirda.
    let g = scen().battlefield(0, &["Zirda, the Dawnwaker", "The Mind Stone", "Plains", "Plains", "Plains", "Plains"]).at(Step::Main1, 0).start();
    let stone = bf(&g, 0, "The Mind Stone");
    assert!(g.has_option(act(stone, 1)), "{{3}}{{W}} with four Plains");
    let g = scen().battlefield(0, &["Zirda, the Dawnwaker", "The Mind Stone", "Plains", "Plains", "Plains"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "The Mind Stone"), 1)));
    let g = scen().battlefield(0, &["The Mind Stone", "Plains", "Plains", "Plains", "Plains"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "The Mind Stone"), 1)), "no Zirda: {{5}}{{W}}");
    // Its own {1}: still one mana.
    let g = scen().battlefield(0, &["Zirda, the Dawnwaker"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(bf(&g, 0, "Zirda, the Dawnwaker"), 0)), "at least one mana");
}

// ---- Thor, God of Thunder --------------------------------------------------

/// "When Thor enters, exile target Equipment, instant, or sorcery card from
/// your graveyard. Until the end of your next turn, you may play that card."
/// "Whenever you cast a noncreature spell, Thor deals damage equal to that
/// spell's mana value to any target."
#[test]
fn thor_exiles_a_spell_to_play_until_the_end_of_your_next_turn_and_burns_for_mana_value() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 7])
        .hand(0, &["Thor, God of Thunder"])
        .graveyard(0, &["Lightning Bolt", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Thor, God of Thunder");
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let bolt = g.card(0, "Lightning Bolt", Zone::Graveyard);
    let bear = g.card(0, "Test Bear", Zone::Graveyard);
    assert!(!g.has_option(g.tgt(bear)), "not a creature card");
    g.choose(g.tgt(bolt));
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    // The opponent's turn and the next own turn: still playable.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 1 && g.turn().step == Step::Upkeep && at_priority(g) && g.pending_decision().unwrap().player == 0);
    assert!(g.has_option(Choice::Cast(bolt)), "during the opponent's turn");
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(g));
    assert!(g.has_option(Choice::Cast(bolt)), "during your next turn");
    // Cast it: Thor's trigger, 1 damage (its mana value).
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    g.choose(Choice::Target(Target::Player(1)));
    settle(&mut g);
    assert_eq!(g.life(1), 20 - 3 - 1, "{}", log(&g));
}

/// The permission ends with your next turn.
#[test]
fn thor_permission_ends_after_your_next_turn() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 5])
        .hand(0, &["Thor, God of Thunder"])
        .graveyard(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, "Thor, God of Thunder");
    settle(&mut g);
    let bolt = g.card(0, "Lightning Bolt", Zone::Exile);
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 3 && g.turn().step == Step::Upkeep && at_priority(g));
    assert!(!g.has_option(Choice::Cast(bolt)), "{}", log(&g));
}

// ---- Jacked Rabbit ---------------------------------------------------------

/// Ravenous (CR 702.156a): "This creature enters with X +1/+1 counters on
/// it. If X is 5 or more, draw a card when it enters." "Whenever this
/// creature attacks, create a number of 1/1 white Rabbit creature tokens
/// equal to this creature's power."
#[test]
fn jacked_rabbit_ravenous_draws_at_x_five_cr702_156a_and_makes_rabbits_equal_to_power() {
    for (x, draws) in [(5u8, 1usize), (4, 0)] {
        let mut g = scen().battlefield(0, &["Plains"; 7]).hand(0, &["Jacked Rabbit"]).at(Step::Main1, 0).start();
        let r = g.card(0, "Jacked Rabbit", Zone::Hand);
        g.choose(Choice::Cast(r));
        g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
        g.choose(Choice::Number(x));
        pay(&mut g);
        let hand = g.player(0).hand.len();
        settle(&mut g);
        assert_eq!(g.power(r), 1 + x as i32);
        assert_eq!(g.player(0).hand.len(), hand + draws, "X = {x}");
    }
    let mut g = scen()
        .battlefield(0, &["Jacked Rabbit"])
        .counters(0, "Jacked Rabbit", engine::db::Counter::P1P1, 2)
        .at(Step::DeclareAttackers, 0)
        .start();
    let r = bf(&g, 0, "Jacked Rabbit");
    g.choose(Choice::Card(r));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(0, "Rabbit", Zone::Battlefield), 3, "power 3: three Rabbits");
}

// ---- Enduring Innocence ----------------------------------------------------

/// "Whenever one or more other creatures you control with power 2 or less
/// enter, draw a card. This ability triggers only once each turn." "When
/// Enduring Innocence dies, if it was a creature, return it to the
/// battlefield under its owner's control. It's an enchantment. (It's not a
/// creature.)"
#[test]
fn enduring_innocence_triggers_only_once_each_turn_and_returns_as_an_enchantment() {
    let mut g = scen()
        .battlefield(0, &["Enduring Innocence", "Plains", "Plains", "Plains", "Plains"])
        .hand(0, &["Test Lifelinker", "Test Cat"])
        .at(Step::Main1, 0)
        .start();
    let hand = g.player(0).hand.len();
    cast(&mut g, "Test Cat");
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand - 1 + 1, "drew one");
    cast(&mut g, "Test Lifelinker");
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand - 1, "once each turn: no second draw");
    // It dies: back as an enchantment, not a creature, no creature types.
    let e = bf(&g, 0, "Enduring Innocence");
    let mut g2 = scen()
        .battlefield(0, &["Enduring Innocence", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let _ = e;
    let e = bf(&g2, 0, "Enduring Innocence");
    let b = g2.card(0, "Lightning Bolt", Zone::Hand);
    g2.choose(Choice::Cast(b));
    g2.choose(g2.tgt(e));
    pay(&mut g2);
    settle(&mut g2);
    assert_eq!(g2.obj(e).zone, Zone::Battlefield, "{}", log(&g2));
    assert!(!g2.is_creature(e) && g2.is_type(e, engine::db::types::ENCHANTMENT));
    assert!(!g2.has_subtype(e, subtype("Sheep")));
}

// ---- Keen-Eyed Curator -----------------------------------------------------

/// "{1}: Exile target card from a graveyard." "As long as there are four or
/// more card types among cards exiled with this creature, it gets +4/+4 and
/// has trample." (CR 607.2a linked; 205.2a card types.)
#[test]
fn keen_eyed_curator_grows_with_four_card_types_among_its_exiled_cards_cr607_2a() {
    let mut g = scen()
        .battlefield(0, &["Keen-Eyed Curator", "Forest", "Forest", "Forest", "Forest"])
        .graveyard(1, &["Lightning Bolt", "Test Bear", "Forest", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let k = bf(&g, 0, "Keen-Eyed Curator");
    for name in ["Lightning Bolt", "Test Bear", "Forest"] {
        let c = g.card(1, name, Zone::Graveyard);
        g.choose(act(k, 0));
        g.choose(g.tgt(c));
        pay(&mut g);
        settle(&mut g);
        assert_eq!(g.obj(c).zone, Zone::Exile);
        assert_eq!(g.power(k), 3, "three types so far");
    }
    let ring = g.card(1, "Sol Ring", Zone::Graveyard);
    g.choose(act(k, 0));
    g.choose(g.tgt(ring));
    pay(&mut g);
    settle(&mut g);
    assert_eq!((g.power(k), g.toughness(k)), (7, 7), "instant, creature, land, artifact");
    assert!(g.has_kw(k, engine::db::Kw::Trample));
}

// ---- Currency Converter ----------------------------------------------------

/// "Whenever you discard a card, you may exile that card from your
/// graveyard." "{2}, {T}: Draw a card, then discard a card." "{T}: Put a
/// card exiled with this artifact into its owner's graveyard. If it's a land
/// card, create a Treasure token. If it's a nonland card, create a 2/2 black
/// Rogue creature token."
#[test]
fn currency_converter_exiles_discards_then_cashes_them_in_for_tokens_cr607_2a() {
    let mut g = scen()
        .battlefield(0, &["Currency Converter", "Island", "Island"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let cc = bf(&g, 0, "Currency Converter");
    g.choose(act(cc, 0));
    pay(&mut g);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Discard));
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Card(bolt));
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Exile);
    // Next turn: {T}: the Bolt goes to the graveyard, a Rogue.
    let t = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number == t + 2 && g.turn().step == Step::Main1 && at_priority(g));
    g.choose(act(cc, 1));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g) || g.kind() == Some(DecisionKind::ChooseExiled));
    if g.kind() == Some(DecisionKind::ChooseExiled) {
        g.choose(Choice::Card(bolt));
    }
    settle(&mut g);
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard);
    assert_eq!(g.count_in(0, "Rogue", Zone::Battlefield), 1, "{}", log(&g));
    assert_eq!(g.count_in(0, "Treasure", Zone::Battlefield), 0);
}
