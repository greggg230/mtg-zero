//! The engineering limits of the action layout (ENGINE.md 58, class B in
//! docs/shortcuts.md): a decision never offers an object outside the action
//! slots that name it. 58.1: a stack taller than the 32 `STACK` slots (a
//! remote self-play worker of a `delver-smoke` run died on `stack slot 38 out
//! of range`, ENGINE.md 56.11): targets are chosen among the 31 stack items
//! below the targeting spell or ability, the deeper (older) items can't be
//! targeted, and a cast or a trigger is offered only with a target in reach.
//! 58.2: 96 permanents a player (a control change or another player's card
//! entering; since ENGINE.md 100.4.8 any number, every one reachable through
//! the pages of the `MY_PERM` / `OPP_PERM` ids). 58.3: the deck limit counts a
//! companion. 65.10: the graveyard
//! cards a choice or a target offers fill the 64 list slots; more fail
//! loudly.

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

//// Every option of the pending decision that names a stack item is a
/// `STACK` id in range (a stack item past the 32 ids is on a page).
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

/// The pending decision's "next page" option (ENGINE.md 100.1).
fn next_page(g: &Game) -> Choice {
    options(g).into_iter().find(|c| matches!(c, Choice::Page(_))).unwrap_or_else(|| panic!("no next page: {:?}", g.options_desc()))
}

/// A determinized world of the decider asks the same page (ENGINE.md 10,
/// 100.1: the page lives in the asking task).
fn same_page_after_determinize(g: &Game) {
    let p = g.pending_decision().unwrap().player;
    let mut d = g.clone();
    d.determinize(p, &mut engine::rng::Rng::new(7));
    assert_eq!(d.pending_decision(), g.pending_decision(), "determinize changed the page");
}

#[test]
fn cr_115_1_a_counterspell_may_target_any_spell_on_a_tall_stack() {
    // 35 Bolts on the stack, then Counterspell on top (slot 0): its targets
    // are all 35 (CR 115.1, 601.2c). The first page offers slots 1..=30 at
    // `STACK` + 1..=30 and "next page" at `STACK` + 31; the second, slots
    // 31..=35 at `STACK` + 0..=4. (Before ENGINE.md 100.2: the 31 below it
    // only, 58.1.)
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
    assert_eq!(actions(&g), (act::STACK + 1..act::STACK + 32).collect::<Vec<_>>(), "{:?}", g.options_desc());
    for k in 1..=30 {
        assert!(options(&g).contains(&g.tgt(spell_at(&g, k))));
    }
    let deep: Vec<CardIdx> = (31..36).map(|k| spell_at(&g, k)).collect();
    g.choose(next_page(&g));
    same_page_after_determinize(&g);
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert_eq!(actions(&g), (act::STACK..act::STACK + 5).collect::<Vec<_>>(), "{:?}", g.options_desc());
    assert_eq!(options(&g), deep.iter().map(|&c| g.tgt(c)).collect::<Vec<_>>());
    stack_ids_in_range(&g);
    // The deepest (the first Bolt cast) is countered.
    let last = deep[4];
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
fn cr_601_2c_a_spell_whose_only_target_is_deep_is_offered() {
    // 30 Bolts: the Elves are 31 below Stern Scolding (`STACK` + 31, one
    // decision). 31 Bolts: 32 below: Stern Scolding may still be cast (CR
    // 601.2c: any legal target), its first page offers "next page" alone and
    // the second the Elves at `STACK` + 1 (slot 32 = 31 + 1). Before ENGINE.md
    // 100.2 it was not offered (58.1).
    let (mut g, elves) = scolding(30);
    let ss = g.card(1, "Stern Scolding", Zone::Hand);
    g.choose(Choice::Cast(ss));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(options(&g), vec![g.tgt(elves)]);
    assert_eq!(actions(&g), vec![act::STACK + 31]);

    let (mut g, elves) = scolding(31);
    let ss = g.card(1, "Stern Scolding", Zone::Hand);
    assert!(at_priority(&g) && g.turn().priority == 1);
    assert!(g.has_option(Choice::Cast(ss)), "{:?}", g.options_desc());
    g.choose(Choice::Cast(ss));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert_eq!(actions(&g), vec![act::STACK + 31]);
    g.choose(next_page(&g));
    assert_eq!(options(&g), vec![g.tgt(elves)]);
    assert_eq!(actions(&g), vec![act::STACK + 1]);
    g.choose(g.tgt(elves));
    finish(&mut g, |_| None);
    g.pass();
    g.pass();
    g.drive_until(at_priority);
    assert_eq!(g.obj(elves).zone, Zone::Graveyard, "countered");
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
fn cr_603_3d_a_triggered_ability_may_target_a_deep_ability() {
    let (g, ping) = tidebinder(30);
    assert_eq!(options(&g), vec![Choice::Pass, Choice::Target(Target::Stack(ping))]);
    assert_eq!(actions(&g), vec![act::PASS, act::STACK + 31]);
    // 32 below: on the second page (CR 603.3d, 115.1).
    let (mut g, ping) = tidebinder(31);
    assert_eq!(actions(&g), vec![act::PASS, act::STACK + 31]);
    g.choose(next_page(&g));
    same_page_after_determinize(&g);
    assert_eq!(options(&g), vec![Choice::Target(Target::Stack(ping))]);
    assert_eq!(actions(&g), vec![act::STACK + 1]);
}

#[test]
fn cr_707_10c_a_copy_may_get_any_new_target_on_a_tall_stack() {
    // P0: Llanowar Elves, Counterspell targeting it, 30 Bolts, then Lutri,
    // whose enters trigger copies the Counterspell ("you may choose new
    // targets for the copy"): the copy goes on top while the trigger
    // resolves (still on the stack, CR 608.2), so the Elves are 33 below it.
    // `PASS` keeps that target; every other spell may be the new one (CR
    // 707.10c, 115.1): the 30 Bolts (slots 2..=31) and the Counterspell (32);
    // the first page has slots 2..=30, the second 31 and 32.
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
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(options(&g).contains(&g.tgt(cs)), "{:?}", g.options_desc());
    g.choose(g.tgt(cs));
    g.drive_until(at_priority);
    g.pass();
    g.pass(); // the trigger resolves: a copy of the Counterspell
    g.drive_until(|g| g.kind() == Some(DecisionKind::NewTarget));
    assert_eq!(g.kind(), Some(DecisionKind::NewTarget), "{:?}", g.options_desc());
    let n = g.stack().len();
    assert!(matches!(g.stack()[n - 2].kind, engine::game::StackKind::Ability { .. }), "the resolving trigger");
    let mut want = vec![act::PASS];
    want.extend(act::STACK + 2..act::STACK + 32);
    assert_eq!(actions(&g), want, "{:?}", g.options_desc());
    assert!(!options(&g).contains(&g.tgt(elves)));
    let bolt31 = spell_at(&g, 31);
    g.choose(next_page(&g));
    same_page_after_determinize(&g);
    assert_eq!(options(&g), vec![g.tgt(bolt31), g.tgt(cs)]);
    assert_eq!(actions(&g), vec![act::STACK, act::STACK + 1]);
    g.choose(g.tgt(cs));
    let copy = g.stack().last().unwrap();
    assert_eq!(copy.targets.iter().map(|t| t.1).collect::<Vec<_>>(), vec![Target::Obj(g.obj_ref(cs))]);
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
/// library.
fn crowded(islands: usize) -> Scenario {
    Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
        .battlefield(0, &["Dack Fayden"])
        .counters(0, "Dack Fayden", engine::db::Counter::Loyalty, 3)
        .battlefield(0, &vec!["Island"; islands])
}

/// docs/shortcuts.md 26.11.6, CR 613.1b: a player may control any number of
/// permanents (ENGINE.md 100.4.8). Dack Fayden's -2 ("gain control of target
/// artifact") takes the Bauble with 120 Islands too (before: with no room
/// left under the engine's 96, nothing happened, ENGINE.md 58.2).
#[test]
fn cr_613_1b_control_of_another_players_permanent_past_96() {
    for islands in [84usize, 85, 120] {
        let mut g = crowded(islands).battlefield(1, &["Mishra's Bauble"]).start();
        let dack = g.card(0, "Dack Fayden", Zone::Battlefield);
        let bauble = g.card(1, "Mishra's Bauble", Zone::Battlefield);
        g.choose(Choice::Ability(dack, AbilitySlot::Activated(1)));
        finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(bauble)));
        g.pass();
        g.pass();
        g.drive_until(at_priority);
        assert_eq!(g.obj(bauble).controller, 0, "{islands} Islands");
        assert_eq!(g.permanents_of(0).count(), islands + 2);
    }
}

/// CR 110.2: Reanimate puts P1's Llanowar Elves onto the battlefield under
/// P0's control however many permanents P0 has (before: under its owner's
/// past the engine's 96).
#[test]
fn cr_110_2_another_players_card_enters_under_the_reanimator_past_96() {
    for islands in [83usize, 84, 120] {
        let mut g = crowded(islands).hand(0, &["Reanimate"]).pool(0, [0, 0, 1, 0, 0, 0], 0).graveyard(1, &["Llanowar Elves"]).start();
        let elves = g.card(1, "Llanowar Elves", Zone::Graveyard);
        let r = g.card(0, "Reanimate", Zone::Hand);
        g.choose(Choice::Cast(r));
        finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then(|| g.tgt(elves)));
        g.pass();
        g.pass();
        g.drive_until(at_priority);
        let e = g.card(1, "Llanowar Elves", Zone::Battlefield);
        assert_eq!(g.obj(e).controller, 0, "{islands} Islands");
    }
}

/// CR 707.10, 111.1: Sage of the Skies ("when you cast this spell, if you've
/// cast another spell this turn, copy it"): the copy of the permanent spell
/// enters as a token however many permanents its controller has (before: it
/// ceased to exist past the engine's 96).
#[test]
fn cr_707_10_a_copy_of_a_permanent_spell_enters_past_96() {
    for islands in [82usize, 83, 120] {
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
        assert_eq!(sages, 2, "{islands} Islands: {}", g.log_lines().join("\n"));
    }
}

/// ENGINE.md 65.10, 100.2: a graveyard-card choice or target is `LIBRARY` +
/// the card's slot in the decision's graveyard list. With offered cards in
/// both graveyards the list holds the offered cards only, the decider's
/// first; past the 64 list slots it is paged (before 100.2: a loud failure,
/// docs/shortcuts.md 65.10). Lion Sash ("{W}: exile target card from a
/// graveyard", any card) with 32 cards in each graveyard: 64 targets,
/// `LIBRARY` .. `LIBRARY` + 63, the other player's last card the last slot;
/// with 33 and 32, the first page holds 63 of them and "next page", the
/// second the other player's last two, at `LIBRARY` + 0 and + 1, and the list
/// block shows those two at those slots (the page's cards).
#[test]
fn cr_115_1_graveyard_targets_past_the_64_list_slots_are_paged_65_10() {
    use engine::Step;
    for (mine, theirs) in [(32usize, 32usize), (33, 32)] {
        let mut g = scen()
            .battlefield(0, &["Lion Sash", "Plains"])
            .graveyard(0, &vec!["Lightning Bolt"; mine])
            .graveyard(1, &vec!["Swamp"; theirs])
            .at(Step::Main1, 0)
            .start();
        let sash = g.card(0, "Lion Sash", Zone::Battlefield);
        let activate = Choice::Ability(sash, AbilitySlot::Activated(0));
        assert!(g.has_option(activate));
        g.choose(activate);
        g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
        let want: Vec<u16> = (0..64).map(|k| act::LIBRARY + k).collect();
        assert_eq!(actions(&g), want, "every card of both graveyards, the decider's first");
        let last = *g.player(1).graveyard.last().unwrap();
        if mine + theirs <= act::LIBRARY_N as usize {
            assert_eq!(g.pending_decision().unwrap().action_of(g.tgt(last)), Some(act::LIBRARY + 63));
        } else {
            assert!(matches!(options(&g)[63], Choice::Page(_)), "{:?}", g.options_desc());
            let two: Vec<CardIdx> = g.player(1).graveyard[30..].to_vec();
            g.choose(next_page(&g));
            same_page_after_determinize(&g);
            assert_eq!(actions(&g), vec![act::LIBRARY, act::LIBRARY + 1]);
            assert_eq!(options(&g), two.iter().map(|&c| g.tgt(c)).collect::<Vec<_>>());
            let d = g.pending_decision().unwrap().clone();
            assert_eq!(g.decision_graveyard_list(0, &d), two.iter().map(|&c| Some(c)).collect::<Vec<_>>());
        }
        g.choose(g.tgt(last));
        finish(&mut g, |_| None);
        g.pass();
        g.pass();
        g.drive_until(at_priority);
        assert_eq!(g.obj(last).zone, Zone::Exile, "{}", g.log_lines().join("\n"));
    }
}

/// The identity a block slot at `o` holds, if present (its card code's three
/// one-hot digits; present: the status's first float).
fn slot_identity(x: &[f32], o: usize) -> Option<usize> {
    use engine::az::encode::CODE;
    let digit = |a: usize, n: usize| x[o + a..o + a + n].iter().position(|&v| v == 1.0);
    (x[o + CODE] == 1.0).then(|| digit(0, 16).unwrap() * 64 + digit(16, 8).unwrap() * 8 + digit(24, 8).unwrap())
}

/// On a page of a choice among stack items, the stack block shows the page's
/// items at their slots (ENGINE.md 100.5.1: `STACK` + j is the item at
/// position 31 + j on the second page, so its token is that item's): with
/// Counterspell on top of 35 Bolts, the second page's slot 0 holds the Bolt
/// 31 below the top, not Counterspell, and slots 5..7 (positions 36..38) are
/// empty. The first page and the other player's encoding show the top.
#[test]
fn cr_115_1_a_page_of_stack_items_is_the_stack_blocks() {
    use engine::az::encode::{O_STACK, STACKS, STACK_W};
    let mut g = scen()
        .hand(0, &["Lightning Bolt"; 35])
        .pool(0, [0, 0, 0, 35, 0, 0], 0)
        .hand(1, &["Counterspell"])
        .pool(1, [0, 2, 0, 0, 0, 0], 0)
        .start();
    bolts(&mut g, 35);
    g.pass();
    let cs = g.card(1, "Counterspell", Zone::Hand);
    g.choose(Choice::Cast(cs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    let ids = engine::az::identity::identities(db());
    let id = |c: CardIdx| ids[g.obj(c).def as usize] as usize;
    let enc = |g: &Game, v: u8| {
        let mut x = vec![0.0f32; engine::az::NUM_FEATURES];
        g.encode_features(v, &mut x);
        x
    };
    let (bolt, counter) = (id(spell_at(&g, 1)), id(cs));
    let x = enc(&g, 1);
    assert_eq!(slot_identity(&x, O_STACK), Some(counter), "the first page: the top");
    g.choose(next_page(&g));
    for v in 0..2u8 {
        let x = enc(&g, v);
        let slots: Vec<Option<usize>> = (0..STACKS).map(|s| slot_identity(&x, O_STACK + s * STACK_W)).collect();
        if v == 1 {
            assert_eq!(slots, [Some(bolt); 5].into_iter().chain([None; 3]).collect::<Vec<_>>(), "P1's page: positions 31..35");
        } else {
            assert_eq!(slots, [Some(counter)].into_iter().chain([Some(bolt); 7]).collect::<Vec<_>>(), "P0 doesn't decide: the top");
        }
    }
}

// ---- 26.11.6: any number of permanents (ENGINE.md 100.4.8) -------------------

/// Choose `c`, following "next page" until it is offered (ENGINE.md 100.1),
/// checking that a determinized world of the decider asks each page again.
/// The pages turned.
fn choose_paged(g: &mut Game, c: Choice) -> usize {
    let mut turned = 0;
    while !g.has_option(c) {
        let next = next_page(g);
        g.choose(next);
        same_page_after_determinize(g);
        turned += 1;
    }
    g.choose(c);
    turned
}

fn enc(g: &Game, v: u8) -> Vec<f32> {
    let mut x = vec![0.0f32; engine::az::NUM_FEATURES];
    g.encode_features(v, &mut x);
    x
}

/// CR 508.1a, 509.1a: attackers and blockers past the 96 `MY_PERM` /
/// `OPP_PERM` ids (docs/shortcuts.md 26.11.6). P0 controls 96 Islands, then
/// Llanowar Elves and Elvish Mystic (positions 96 and 97): the attackers'
/// first page offers `PASS` and "next page" (`MY_PERM` + 95), the second the
/// Elves and the Mystic at `MY_PERM` + 1 and + 2 (positions 95 + j), and the
/// decider's permanent blocks show positions 95 on there (its block 1 is the
/// Elves, with its option flag). Both attack; P1's Elves block the Mystic,
/// also past the `OPP_PERM` ids. (Before: a loud failure, "permanent slot 96
/// out of range", and no player could have 97 permanents.)
#[test]
fn cr_508_1a_attackers_and_blockers_past_96_permanents() {
    use engine::az::encode::{perm, CODE, O_MY_PERM, PERM_W};
    use engine::Step;
    let mut mine = vec!["Island"; 96];
    mine.extend(["Llanowar Elves", "Elvish Mystic"]);
    let mut g = scen().battlefield(0, &mine).battlefield(1, &["Llanowar Elves"]).at(Step::Main1, 0).start();
    let elves = g.card(0, "Llanowar Elves", Zone::Battlefield);
    let birds = g.card(0, "Elvish Mystic", Zone::Battlefield);
    let blocker = g.card(1, "Llanowar Elves", Zone::Battlefield);
    assert_eq!((g.perm_slot(elves), g.perm_slot(birds)), (96, 97));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    assert_eq!(actions(&g), vec![act::PASS, act::MY_PERM + 95], "{:?}", g.options_desc());
    g.choose(next_page(&g));
    same_page_after_determinize(&g);
    assert_eq!(actions(&g), vec![act::MY_PERM + 1, act::MY_PERM + 2], "{:?}", g.options_desc());
    let ids = engine::az::identity::identities(db());
    let x = enc(&g, 0);
    assert_eq!(slot_identity(&x, O_MY_PERM + PERM_W), Some(ids[g.obj(elves).def as usize] as usize), "block 1: position 96");
    assert_eq!(x[O_MY_PERM + PERM_W + CODE + perm::OPTION], 1.0);
    assert_eq!(x[O_MY_PERM + CODE + perm::OPTION], 0.0, "block 0: the Island at position 95");
    assert_eq!(slot_identity(&x, O_MY_PERM + 3 * PERM_W), None, "no position 98");
    g.choose(Choice::Card(elves));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    choose_paged(&mut g, Choice::Card(birds));
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Pass);
    g.drive_until(|g| g.kind() == Some(DecisionKind::DeclareBlockers));
    assert_eq!(g.pending_decision().unwrap().player, 1);
    assert_eq!(choose_paged(&mut g, Choice::Card(birds)), 1, "the Mystic on the second page: {:?}", g.options_desc());
    g.drive_until(|g| g.turn().step == Step::Main2 || g.is_over());
    assert_eq!(g.life(1), 19, "the Elves unblocked: {}", g.log_lines().join("\n"));
    assert_eq!(g.obj(blocker).zone, Zone::Graveyard, "the Mystic and its blocker trade: {}", g.log_lines().join("\n"));
    assert_eq!(g.obj(birds).zone, Zone::Graveyard);
}

/// CR 602.2, 117.1b: an activated ability of a permanent past the 96 slots is
/// offered at priority, its `ACTIVATE` id on the page of the decider's
/// permanents (`MY_PERM` + 95 = "next page" there; ENGINE.md 100.4.8): 96
/// Islands, then Mishra's Bauble (position 96): the second page offers its
/// "{T}, Sacrifice" at `ACTIVATE` + 4, and the permanent blocks show it at
/// block 1 with its `ACTIVATE` flag.
#[test]
fn cr_602_2_an_ability_of_a_permanent_past_96_is_offered_at_priority() {
    use engine::az::encode::{perm, CODE, O_MY_PERM, PERM_W};
    use engine::Step;
    let mut mine = vec!["Island"; 96];
    mine.push("Mishra's Bauble");
    let mut g = scen().battlefield(0, &mine).at(Step::Main1, 0).start();
    let bauble = g.card(0, "Mishra's Bauble", Zone::Battlefield);
    let activate = Choice::Ability(bauble, AbilitySlot::Activated(0));
    assert!(!g.has_option(activate));
    assert!(actions(&g).contains(&(act::MY_PERM + 95)), "{:?}", g.options_desc());
    g.choose(next_page(&g));
    same_page_after_determinize(&g);
    assert_eq!(g.pending_decision().unwrap().action_of(activate), Some(act::ACTIVATE + 4), "{:?}", g.options_desc());
    let x = enc(&g, 0);
    assert_eq!(x[O_MY_PERM + PERM_W + CODE + perm::ACTIVATE], 1.0, "block 1: the Bauble's ability");
    assert_eq!(x[O_MY_PERM + CODE + perm::ACTIVATE], 0.0);
    g.choose(activate);
    finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
    assert_eq!(g.obj(bauble).zone, Zone::Graveyard, "{}", g.log_lines().join("\n"));
}

/// CR 601.2g: a mana source past the 96 slots pays (ENGINE.md 100.4.8): 96
/// Islands, then a Mountain (position 96), Lightning Bolt in hand. The
/// payment's first page offers only "next page"; the Mountain's ability is on
/// the second. With `auto_single` (as games are played), the engine answers
/// both one-option pages and the Bolt is cast.
#[test]
fn cr_601_2g_a_mana_source_past_96_permanents_pays() {
    use engine::Step;
    let mut mine = vec!["Island"; 96];
    mine.push("Mountain");
    for auto in [false, true] {
        let mut g = scen().spec(|s| s.auto_single = auto).battlefield(0, &mine).hand(0, &["Lightning Bolt"]).at(Step::Main1, 0).start();
        let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
        let mountain = g.card(0, "Mountain", Zone::Battlefield);
        g.choose(Choice::Cast(bolt));
        if !auto {
            g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
            g.choose(player(1));
            assert_eq!(g.kind(), Some(DecisionKind::PayMana));
            assert_eq!(actions(&g), vec![act::MY_PERM + 95], "{:?}", g.options_desc());
            assert_eq!(choose_paged(&mut g, Choice::Ability(mountain, AbilitySlot::Activated(0))), 1);
        } else {
            finish(&mut g, |g| (g.kind() == Some(DecisionKind::Target)).then_some(player(1)));
        }
        g.drive_until(|g| g.stack().is_empty() && at_priority(g));
        assert_eq!(g.life(1), 17, "auto {auto}: {}", g.log_lines().join("\n"));
        assert!(g.obj(mountain).tapped);
    }
}
