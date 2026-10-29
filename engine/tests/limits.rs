//! The engineering limits of the action layout (ENGINE.md 58, class B in
//! docs/shortcuts.md): a decision never offers an object outside the action
//! slots that name it. 58.1: a stack taller than the 32 `STACK` slots (a
//! remote self-play worker of a `delver-smoke` run died on `stack slot 38 out
//! of range`, ENGINE.md 56.11): targets are chosen among the 31 stack items
//! below the targeting spell or ability, the deeper (older) items can't be
//! targeted, and a cast or a trigger is offered only with a target in reach.
//! 58.2: 96 permanents a player (a control change or another player's card
//! entering). 58.3: the deck limit counts a companion.

mod common;

use common::db;
use engine::az::constructed;
use engine::db::AbilitySlot;
use engine::decision::act;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Target, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

fn actions(g: &Game) -> Vec<u16> {
    g.pending_decision().unwrap().options.iter().map(|o| o.action).collect()
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

/// Answer the decisions of a cast or activation (`answer`, else the first
/// option) until a player has priority again.
fn finish(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..100 {
        if at_priority(g) {
            return;
        }
        let c = answer(g).unwrap_or_else(|| g.pending_decision().unwrap().options[0].choice);
        g.choose(c);
    }
    panic!("finish: no progress");
}

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 10]).library(1, &["Island"; 10])
}

/// P0 casts `n` Lightning Bolts at P1, holding priority (CR 117.3c).
fn bolts(g: &mut Game, n: usize) {
    for _ in 0..n {
        assert_eq!(g.turn().priority, 0);
        let b = g.card(0, "Lightning Bolt", Zone::Hand);
        g.choose(Choice::Cast(b));
        finish(g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    }
}

/// Every option of the pending decision that names a stack item is a
/// `STACK` id in range (the property 58.1 keeps).
fn stack_ids_in_range(g: &Game) {
    for a in actions(g) {
        if (act::STACK..act::TRIGGER).contains(&a) {
            assert!(a < act::STACK + act::STACK_N);
        }
    }
}

/// The card of the spell `k` items from the top of the stack.
fn spell_at(g: &Game, from_top: usize) -> CardIdx {
    let s = g.stack();
    match s[s.len() - 1 - from_top].kind {
        engine::game::StackKind::Spell { card } => card,
        k => panic!("not a spell: {k:?}"),
    }
}

#[test]
fn a_counterspell_targets_only_the_31_spells_below_it() {
    // 35 Bolts on the stack: Counterspell, on top, may target the 31 below
    // it (`STACK` + 1 ..= + 31); the 4 oldest can't be targeted.
    let mut g = scen()
        .hand(0, &["Lightning Bolt"; 35])
        .pool(0, [0, 0, 0, 35, 0, 0], 0)
        .hand(1, &["Counterspell"])
        .pool(1, [0, 2, 0, 0, 0, 0], 0)
        .start();
    bolts(&mut g, 35);
    assert_eq!(g.stack().len(), 35);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    assert!(g.has_option(Choice::Cast(cs)));
    g.choose(Choice::Cast(cs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let acts = actions(&g);
    assert_eq!(acts, (act::STACK + 1..act::STACK + 32).collect::<Vec<_>>(), "{:?}", g.options_desc());
    stack_ids_in_range(&g);
    // (The stack: the 35 Bolts, then Counterspell; the 4 oldest out of reach.)
    let deep: Vec<CardIdx> = (32..36).map(|k| spell_at(&g, k)).collect();
    for c in &deep {
        assert!(!options(&g).contains(&g.tgt(*c)), "a Bolt beyond slot 31 is offered");
    }
    // The deepest in reach (slot 31) is countered.
    let last = spell_at(&g, 31);
    g.choose(g.tgt(last));
    finish(&mut g, |_| None);
    g.pass();
    g.pass();
    g.drive_until(at_priority);
    assert_eq!(g.obj(last).zone, Zone::Graveyard);
    assert_eq!(g.stack().len(), 34);
}

/// P0 casts Llanowar Elves (a 1/1 creature spell), then `n` Bolts on it;
/// P1 has Stern Scolding ("counter target creature spell with power or
/// toughness 2 or less"): its only legal target is the Elves, `n` + 1
/// items below it once it is cast.
fn scolding(n: usize) -> (Game, CardIdx) {
    let mut g = scen()
        .hand(0, &["Llanowar Elves"])
        .hand(0, &vec!["Lightning Bolt"; n])
        .pool(0, [0, 0, 0, n as u8, 1, 0], 0)
        .hand(1, &["Stern Scolding"])
        .pool(1, [0, 1, 0, 0, 0, 0], 0)
        .start();
    let elves = g.card(0, "Llanowar Elves", Zone::Hand);
    g.choose(Choice::Cast(elves));
    finish(&mut g, |_| None);
    bolts(&mut g, n);
    g.pass();
    (g, elves)
}

#[test]
fn a_spell_whose_only_target_is_beyond_reach_is_not_offered() {
    // 30 Bolts: the Elves are 31 below Stern Scolding (in reach, `STACK` +
    // 31), so it may be cast; 31 Bolts: 32 below, not targetable, and Stern
    // Scolding is not offered (a cast is offered only if it can be completed,
    // ENGINE.md 6). Before 58.1 the cast was offered and choosing its target
    // panicked ("stack slot 32 out of range").
    let (mut g, elves) = scolding(30);
    let ss = g.card(1, "Stern Scolding", Zone::Hand);
    assert!(g.has_option(Choice::Cast(ss)));
    g.choose(Choice::Cast(ss));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(options(&g), vec![g.tgt(elves)]);
    assert_eq!(actions(&g), vec![act::STACK + 31]);

    let (g, _) = scolding(31);
    let ss = g.card(1, "Stern Scolding", Zone::Hand);
    assert!(at_priority(&g) && g.turn().priority == 1);
    assert!(!g.has_option(Choice::Cast(ss)), "{:?}", g.options_desc());
}

/// P0 activates Mishra's Bauble (an ability on the stack, targeting P1),
/// then casts `n` Bolts; P1 flashes in Tishana's Tidebinder, which resolves:
/// its trigger ("counter up to one target activated or triggered ability")
/// chooses its target with the Bauble's ability `n` + 1 items below it.
fn tidebinder(n: usize) -> (Game, u32) {
    let mut g = scen()
        .battlefield(0, &["Mishra's Bauble"])
        .hand(0, &vec!["Lightning Bolt"; n])
        .pool(0, [0, 0, 0, n as u8, 0, 0], 0)
        .hand(1, &["Tishana's Tidebinder"])
        .battlefield(1, &["Island"; 3])
        .start();
    let bauble = g.card(0, "Mishra's Bauble", Zone::Battlefield);
    g.choose(Choice::Ability(bauble, AbilitySlot::Activated(0)));
    finish(&mut g, |g| g.has_option(player(1)).then_some(player(1)));
    let ping = g.stack()[0].id;
    bolts(&mut g, n);
    g.pass();
    let tb = g.card(1, "Tishana's Tidebinder", Zone::Hand);
    g.choose(Choice::Cast(tb));
    finish(&mut g, |_| None);
    g.pass();
    g.pass(); // Tidebinder resolves; its trigger chooses a target
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    (g, ping)
}

#[test]
fn a_triggered_ability_targets_an_ability_only_in_reach() {
    let (g, ping) = tidebinder(30);
    assert_eq!(options(&g), vec![Choice::Pass, Choice::Target(Target::Stack(ping))]);
    assert_eq!(actions(&g), vec![act::PASS, act::STACK + 31]);
    let (g, _) = tidebinder(31);
    assert_eq!(options(&g), vec![Choice::Pass], "the ability 32 below can't be targeted");
}

#[test]
fn a_copy_keeps_a_deep_target_and_offers_new_ones_in_reach() {
    // P0: Llanowar Elves, Counterspell targeting it, 30 Bolts, then Lutri,
    // whose enters trigger copies the Counterspell ("you may choose new
    // targets for the copy"): the copy goes on top while the trigger
    // resolves (still on the stack, CR 608.2), so the Elves are 33 below it.
    // `PASS` keeps that target; the new targets offered are the spells among
    // the 31 items below the copy: the 30 Bolts (the trigger is an ability,
    // the Counterspell 32 below).
    let mut g = scen()
        .hand(0, &["Llanowar Elves", "Counterspell", "Lutri, the Spellchaser"])
        .hand(0, &["Lightning Bolt"; 30])
        .pool(0, [0, 5, 0, 30, 1, 0], 0)
        .start();
    let elves = g.card(0, "Llanowar Elves", Zone::Hand);
    g.choose(Choice::Cast(elves));
    finish(&mut g, |_| None);
    let cs = g.card(0, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(elves)));
    bolts(&mut g, 30);
    let lutri = g.card(0, "Lutri, the Spellchaser", Zone::Hand);
    g.choose(Choice::Cast(lutri));
    finish(&mut g, |_| None);
    g.pass();
    g.pass(); // Lutri resolves
    // Its trigger's target: the Counterspell, 31 below it (in reach).
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(options(&g).contains(&g.tgt(cs)), "{:?}", g.options_desc());
    stack_ids_in_range(&g);
    g.choose(g.tgt(cs));
    g.drive_until(at_priority);
    g.pass();
    g.pass(); // the trigger resolves: a copy of the Counterspell
    g.drive_until(|g| g.kind() == Some(DecisionKind::NewTarget));
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{:?}", g.options_desc());
    stack_ids_in_range(&g);
    let n = g.stack().len();
    assert!(matches!(g.stack()[n - 2].kind, engine::game::StackKind::Ability { .. }), "the resolving trigger");
    let acts = actions(&g);
    assert_eq!(acts[0], act::PASS);
    assert_eq!(&acts[1..], &(act::STACK + 2..act::STACK + 32).collect::<Vec<_>>()[..], "{:?}", g.options_desc());
    assert!(!options(&g).contains(&g.tgt(elves)) && !options(&g).contains(&g.tgt(cs)));
    g.pass();
    let copy = g.stack().last().unwrap();
    assert_eq!(copy.targets.iter().map(|t| t.1).collect::<Vec<_>>(), vec![Target::Obj(g.obj_ref(elves))]);
}

#[test]
fn the_deck_limit_counts_a_companion_58_3() {
    // 64 cards: the engine's most (a hand, a library list, a graveyard have
    // 64 action slots); with a companion in the sideboard, which can join
    // the game, 63.
    let d = engine::db::CardDb::constructed();
    let island = d.expect_id("Island").unwrap();
    let lurrus = d.expect_id("Lurrus of the Dream-Den").unwrap();
    let deck = vec![island; 64];
    assert!(constructed::check_size(d, &deck, &[]).is_ok());
    assert!(constructed::check_size(d, &deck, &[lurrus]).is_err());
    assert!(constructed::check_size(d, &deck[..63], &[lurrus]).is_ok());
    // A limited list too (no maximum in CR 100.2a, the engine's 64).
    let names: Vec<&str> = vec!["Island"; 65];
    let spec = serde_json::json!({"decks": {"p0": names, "p1": names}}).to_string();
    let e = engine::az::MtgSpec::parse(&spec).unwrap_err();
    assert!(e.contains("at most 64"), "{e}");
    let names: Vec<&str> = vec!["Island"; 64];
    assert!(engine::az::MtgSpec::parse(&serde_json::json!({"decks": {"p0": names, "p1": names}}).to_string()).is_ok());
}

/// P0 controls Dack Fayden and `islands` Islands, with ten cards in their
/// library: room (`perm_room`) for 96 - (`islands` + 1) - 10 - (their other
/// cards: in hand, on the stack, in a graveyard) more permanents that
/// aren't their own cards.
fn crowded(islands: usize) -> Scenario {
    Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Dack Fayden"])
        .counters(0, "Dack Fayden", engine::db::Counter::Loyalty, 3)
        .battlefield(0, &vec!["Island"; islands])
}

#[test]
fn no_control_of_another_players_permanent_beyond_96_58_2() {
    // Dack Fayden's -2 ("gain control of target artifact"): with room for one
    // more permanent the Bauble changes control; with none, nothing happens
    // (ENGINE.md 58.2), and P0 never controls more than 96 permanents.
    for (islands, gained) in [(84usize, true), (85, false)] {
        let mut g = crowded(islands).battlefield(1, &["Mishra's Bauble"]).start();
        let dack = g.card(0, "Dack Fayden", Zone::Battlefield);
        let bauble = g.card(1, "Mishra's Bauble", Zone::Battlefield);
        g.choose(Choice::Ability(dack, AbilitySlot::Activated(1)));
        finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bauble)));
        g.pass();
        g.pass();
        g.drive_until(at_priority);
        assert_eq!(g.obj(bauble).controller == 0, gained, "{islands} Islands");
        assert!(g.permanents_of(0).count() <= act::PERM_N as usize);
    }
}

#[test]
fn another_players_card_enters_under_its_owner_beyond_96_58_2() {
    // Reanimate on P1's Llanowar Elves: under P0's control with room, under
    // its owner's without (its owner's room keeps it).
    for (islands, mine) in [(83usize, true), (84, false)] {
        let mut g = crowded(islands).hand(0, &["Reanimate"]).pool(0, [0, 0, 1, 0, 0, 0], 0).graveyard(1, &["Llanowar Elves"]).start();
        let elves = g.card(1, "Llanowar Elves", Zone::Graveyard);
        let r = g.card(0, "Reanimate", Zone::Hand);
        g.choose(Choice::Cast(r));
        finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(elves)));
        g.pass();
        g.pass();
        g.drive_until(at_priority);
        let e = g.card(1, "Llanowar Elves", Zone::Battlefield);
        assert_eq!(g.obj(e).controller == 0, mine, "{islands} Islands");
        assert!(g.permanents_of(0).count() <= act::PERM_N as usize);
    }
}

#[test]
fn a_copy_of_a_permanent_spell_beyond_96_ceases_to_exist_58_2() {
    // Sage of the Skies ("when you cast this spell, if you've cast another
    // spell this turn, copy it"; a copy of a permanent spell becomes a token,
    // CR 707.10, 111.1): the copy enters with room, else it ceases to exist;
    // the card enters either way (its owner's room keeps it).
    for (islands, copies) in [(82usize, 1usize), (83, 0)] {
        let mut g = crowded(islands)
            .hand(0, &["Lightning Bolt", "Sage of the Skies"])
            .pool(0, [1, 0, 0, 1, 0, 2], 0)
            .start();
        bolts(&mut g, 1);
        g.pass();
        g.pass(); // the Bolt resolves: Sage is cast at sorcery speed
        g.drive_until(at_priority);
        let sage = g.card(0, "Sage of the Skies", Zone::Hand);
        g.choose(Choice::Cast(sage));
        finish(&mut g, |_| None);
        for _ in 0..40 {
            g.drive_until(|g| g.stack().is_empty() && at_priority(g));
            if g.stack().is_empty() {
                break;
            }
            let c = g.pending_decision().unwrap().options[0].choice;
            g.choose(c);
        }
        let sages = g.permanents_of(0).filter(|&c| g.name(c) == "Sage of the Skies").count();
        assert_eq!(sages, 1 + copies, "{islands} Islands: {}", g.log_lines().join("\n"));
        assert!(g.permanents_of(0).count() <= act::PERM_N as usize);
    }
}
