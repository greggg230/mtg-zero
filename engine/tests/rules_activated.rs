//! Activated abilities, costs and mana abilities (CR 602, 605, 118, 119.4,
//! 107.4, 302.6, 601.2b).

mod common;

use common::db;
use engine::db::AbilitySlot;
use engine::hidden::{key_kind, key_player, KEY_REVEAL};
use engine::mana::{ManaCost, B, C, G};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Step, Target, Zone};

fn manual() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 8]).library(1, &["Island"; 8])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn settle(g: &mut Game) {
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
}

fn cost(s: &str) -> ManaCost {
    ManaCost::parse(s).unwrap()
}

/// Pass priority, attack and block with nothing, and answer forced
/// decisions until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ if d.options.len() == 1 => d.options[0].choice,
            k => panic!("quiet_until: unexpected {k:?}"),
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

#[test]
fn cr_602_2a_the_ability_is_on_the_stack_before_targets_and_costs() {
    let mut g = manual().battlefield(0, &["Mountain", "Pyrite Spellbomb"]).at(Step::Main1, 0).start();
    let bomb = g.card(0, "Pyrite Spellbomb", Zone::Battlefield);
    // {R}, sacrifice: 2 damage to any target; {1}, sacrifice: draw.
    assert!(g.has_option(act(bomb, 0)) && g.has_option(act(bomb, 1)));
    g.choose(act(bomb, 0));
    // 602.2a: it is on the stack; 601.2c: targets come next, before costs.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert_eq!(g.stack().len(), 1);
    assert_eq!(g.obj(bomb).zone, Zone::Battlefield);
    g.choose(Choice::Target(Target::Player(1)));
    // 601.2g-h: the mana, then the sacrifice.
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Pyrite Spellbomb", Zone::Graveyard), 1);
    assert_eq!(g.stack().len(), 1);
    assert_eq!(g.to_move(), Some(0), "117.3c: the activator receives priority");
    settle(&mut g);
    assert_eq!(g.life(1), 18);
}

#[test]
fn cr_602_2_targets_are_chosen_before_a_sacrifice_cost_is_paid() {
    // Goblin Bombardment may target the creature it will sacrifice; the
    // ability then has no legal target and does not resolve (608.2b).
    let mut g = manual().battlefield(0, &["Goblin Bombardment", "Test Bear"]).at(Step::Main1, 0).start();
    let gb = g.card(0, "Goblin Bombardment", Zone::Battlefield);
    let bear = g.card(0, "Test Bear", Zone::Battlefield);
    g.choose(act(gb, 0));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    g.choose(Choice::Card(bear));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    settle(&mut g);
    assert_eq!((g.life(0), g.life(1)), (20, 20));
    assert!(g.log_lines().iter().any(|l| l.contains("fizzles")));

    // Targeting the opponent instead: 1 damage.
    let mut g = manual().battlefield(0, &["Goblin Bombardment", "Test Bear"]).at(Step::Main1, 0).start();
    let gb = g.card(0, "Goblin Bombardment", Zone::Battlefield);
    g.choose(act(gb, 0));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    settle(&mut g);
    assert_eq!(g.life(1), 19);
}

#[test]
fn cr_602_2_a_sacrificed_source_is_gone_while_its_ability_waits_and_resolves() {
    let mut g = manual().battlefield(0, &["Mind Stone", "Island"]).at(Step::Main1, 0).start();
    let stone = g.card(0, "Mind Stone", Zone::Battlefield);
    // Ability 0 is the {T}: Add {C} mana ability: not a priority action.
    assert!(!g.has_option(act(stone, 0)));
    g.choose(act(stone, 1)); // {1}, {T}, sacrifice: draw a card
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    // Mind Stone cannot pay its own {1}: its cost taps it.
    assert_eq!(g.options_desc().len(), 1, "{:?}", g.options_desc());
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Mind Stone", Zone::Graveyard), 1);
    assert_eq!(g.stack().len(), 1);
    let hand = g.player(0).hand.len();
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), hand + 1, "113.7a: it resolves without its source");
}

#[test]
fn cr_605_mana_abilities_are_activated_while_paying_not_at_priority() {
    let mut g = manual()
        .battlefield(0, &["Llanowar Elves", "Mountain", "Mox Pearl"])
        .hand(0, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let elves = g.card(0, "Llanowar Elves", Zone::Battlefield);
    let mox = g.card(0, "Mox Pearl", Zone::Battlefield);
    assert!(!g.has_option(act(elves, 0)) && !g.has_option(act(mox, 0)));
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear)); // {1}{G}
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    assert!(g.has_option(act(elves, 0)));
    g.choose(act(elves, 0));
    g.choose(act(mox, 0));
    assert!(at_priority(&g));
    assert!(g.obj(elves).tapped && g.obj(mox).tapped);
    // 605.3a: mana abilities do not use the stack.
    assert_eq!(g.stack().len(), 1);
}

#[test]
fn cr_302_6_tap_abilities_of_creatures_need_continuous_control() {
    let mut g = manual()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Mountain", "Forest"])
        .hand(0, &["Test Tapper", "Test Hasty Tapper", "Llanowar Elves"])
        .at(Step::Main1, 0)
        .start();
    for name in ["Test Tapper", "Test Hasty Tapper", "Llanowar Elves"] {
        let c = g.card(0, name, Zone::Hand);
        g.choose(Choice::Cast(c));
        while g.kind() == Some(DecisionKind::PayMana) {
            let first = g.pending_decision().unwrap().options[0].choice;
            g.choose(first);
        }
        settle(&mut g);
    }
    let tapper = g.card(0, "Test Tapper", Zone::Battlefield);
    let hasty = g.card(0, "Test Hasty Tapper", Zone::Battlefield);
    assert!(!g.has_option(act(tapper, 0)), "summoning sick");
    assert!(g.has_option(act(hasty, 0)), "702.10: haste");
    // The new Elves are not a mana source yet; every land is tapped.
    assert!(!g.can_pay(0, &cost("{G}")));
    // Next turn they are.
    quiet_until(&mut g, |g| g.turn().number == 5 && g.turn().step == Step::Main1);
    let elves = g.card(0, "Llanowar Elves", Zone::Battlefield);
    assert!(g.has_option(act(tapper, 0)));
    let _ = elves;
    assert!(g.can_pay(0, &cost("{5}{G}")));
}

#[test]
fn cr_302_6_abilities_without_tap_and_noncreature_tap_abilities_work_at_once() {
    let mut g = manual()
        .battlefield(0, &["Swamp"; 8])
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Griselbrand", "Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    for name in ["Griselbrand", "Mind Stone"] {
        let c = g.card(0, name, Zone::Hand);
        g.choose(Choice::Cast(c));
        while g.kind() == Some(DecisionKind::PayMana) {
            let first = g.pending_decision().unwrap().options[0].choice;
            g.choose(first);
        }
        settle(&mut g);
    }
    let gris = g.card(0, "Griselbrand", Zone::Battlefield);
    let stone = g.card(0, "Mind Stone", Zone::Battlefield);
    assert!(g.has_option(act(gris, 0)), "pay 7 life: no {{T}} in the cost");
    assert!(g.has_option(act(stone, 1)), "an artifact is not summoning sick");
}

#[test]
fn cr_602_5d_activate_only_as_a_sorcery() {
    let mut g = manual()
        .battlefield(0, &["Guerrilla Gorilla", "Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    let gorilla = g.card(0, "Guerrilla Gorilla", Zone::Battlefield);
    assert!(g.has_option(act(gorilla, 0)));
    // Not with a spell on the stack.
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    assert!(!g.has_option(act(gorilla, 0)));
    settle(&mut g);
    assert!(g.has_option(act(gorilla, 0)));
    // Not in the opponent's turn.
    g.drive_until(|g| g.turn().active == 1 && g.turn().step == Step::Upkeep && g.to_move() == Some(0));
    assert!(!g.has_option(act(gorilla, 0)));
}

#[test]
fn cr_602_5b_activate_only_if() {
    // Library of Alexandria: exactly seven cards in hand.
    let mut g = manual()
        .battlefield(0, &["Library of Alexandria"])
        .hand(0, &["Island"; 7])
        .at(Step::Upkeep, 0)
        .start();
    let lib = g.card(0, "Library of Alexandria", Zone::Battlefield);
    assert!(g.has_option(act(lib, 1)));
    g.choose(act(lib, 1));
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 8);
    assert!(!g.has_option(act(lib, 1)));

    // Mox Opal: only with three or more artifacts.
    let g = manual().battlefield(0, &["Mox Opal", "Mind Stone"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{W}")));
    let g = manual().battlefield(0, &["Mox Opal", "Mind Stone", "Sol Ring"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{W}")));

    // Blazemire Verge: {R} only with a Swamp or a Mountain.
    let g = manual().battlefield(0, &["Blazemire Verge"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{B}")) && !g.can_pay(0, &cost("{R}")));
    let g = manual().battlefield(0, &["Blazemire Verge", "Badlands"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{R}{R}")), "Badlands is a Swamp Mountain");
}

#[test]
fn cr_106_several_mana_sources_leave_their_excess_in_the_pool() {
    // Sol Ring pays {1}; the other {C} stays.
    let mut g = manual().battlefield(0, &["Sol Ring"]).hand(0, &["Test Zero"]).at(Step::Main1, 0).start();
    let z = g.card(0, "Test Zero", Zone::Hand);
    g.choose(Choice::Cast(z));
    g.drive_until(at_priority);
    assert_eq!(g.player(0).pool.amt[C], 1);

    // Black Lotus: three mana of one colour; only green keeps {1}{G} payable.
    let mut g = manual().battlefield(0, &["Black Lotus"]).hand(0, &["Test Bear"]).at(Step::Main1, 0).start();
    let lotus = g.card(0, "Black Lotus", Zone::Battlefield);
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    g.choose(act(lotus, 0));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana));
    assert_eq!(g.options_desc().len(), 1, "only GGG pays {{1}}{{G}}");
    g.drive_until(at_priority);
    assert_eq!(g.player(0).pool.amt[G], 1);
    assert_eq!(g.count_in(0, "Black Lotus", Zone::Graveyard), 1);

    // With a Forest for the {G}, any colour is possible.
    let mut g =
        manual().battlefield(0, &["Black Lotus", "Forest"]).hand(0, &["Test Bear"]).at(Step::Main1, 0).start();
    let lotus = g.card(0, "Black Lotus", Zone::Battlefield);
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    g.choose(act(lotus, 0));
    assert_eq!(g.options_desc().len(), 5);
}

#[test]
fn cr_601_2g_a_conditional_source_is_activated_before_a_sacrificed_one() {
    // Mox Opal needs three artifacts; Black Lotus sacrifices itself. Paying
    // {3}{W} with both is legal (activate Opal first).
    let mut g = manual()
        .battlefield(0, &["Mox Opal", "Black Lotus", "Mind Stone"])
        .hand(0, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let giant = g.card(0, "Test Giant", Zone::Hand); // {4}{G}
    assert!(g.has_option(Choice::Cast(giant)));
    g.choose(Choice::Cast(giant));
    let lotus = g.card(0, "Black Lotus", Zone::Battlefield);
    g.choose(act(lotus, 0));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    while !at_priority(&g) {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
    assert_eq!(g.stack().len(), 1);
    assert_eq!(g.count_in(0, "Black Lotus", Zone::Graveyard), 1);
    assert!(g.obj(g.card(0, "Mox Opal", Zone::Battlefield)).tapped);
}

#[test]
fn a_cards_mana_abilities_are_alternatives() {
    // A Talisman taps for {C} or for one of two colours (dealing 1 damage),
    // never both.
    let g = manual().battlefield(0, &["Talisman of Dominance"]).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}")) && !g.can_pay(0, &cost("{1}{U}")));
    let mut g = manual()
        .battlefield(0, &["Talisman of Dominance", "Plains"])
        .hand(0, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let t = g.card(0, "Talisman of Dominance", Zone::Battlefield);
    let f = g.card(0, "Test Flyer", Zone::Hand);
    g.choose(Choice::Cast(f)); // {1}{U}
    // The {C} ability cannot help ({U} must come from the Talisman).
    assert!(!g.has_option(act(t, 0)) && g.has_option(act(t, 1)));
    g.choose(act(t, 1));
    g.drive_until(at_priority);
    assert_eq!(g.life(0), 19, "the coloured ability deals 1 damage to you");
}

#[test]
fn cr_119_4_life_can_be_paid_only_up_to_the_life_total() {
    // Mana Confluence at 1 life can pay (going to 0).
    let mut g =
        manual().battlefield(0, &["Mana Confluence"]).hand(0, &["Test Zero"]).life(0, 1).at(Step::Main1, 0).start();
    let z = g.card(0, "Test Zero", Zone::Hand);
    assert!(g.has_option(Choice::Cast(z)));
    g.choose(Choice::Cast(z));
    g.drive_until(|g| g.is_over() || at_priority(g));
    assert!(g.is_over(), "0 life: the game ends at the next SBA check");
    // Two Confluences at 1 life pay for one mana only.
    let g = manual().battlefield(0, &["Mana Confluence", "Mana Confluence"]).life(0, 1).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{U}")) && !g.can_pay(0, &cost("{1}{U}")));
    let g = manual().battlefield(0, &["Mana Confluence", "Mana Confluence"]).life(0, 2).at(Step::Main1, 0).start();
    assert!(g.can_pay(0, &cost("{1}{U}")));
    // Griselbrand's "Pay 7 life" at exactly 7 life (the Python engine
    // requires more).
    let g = manual().battlefield(0, &["Griselbrand"]).life(0, 7).at(Step::Main1, 0).start();
    let gris = g.card(0, "Griselbrand", Zone::Battlefield);
    assert!(g.has_option(act(gris, 0)));
    let g = manual().battlefield(0, &["Griselbrand"]).life(0, 6).at(Step::Main1, 0).start();
    assert!(!g.has_option(act(gris, 0)));
}

#[test]
fn cr_107_4f_phyrexian_mana_is_a_choice() {
    let mut g = manual()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Test Phyrexian Shock"])
        .at(Step::Main1, 0)
        .start();
    let s = g.card(0, "Test Phyrexian Shock", Zone::Hand);
    g.choose(Choice::Cast(s));
    g.choose(Choice::Target(Target::Player(1)));
    assert_eq!(g.kind(), Some(DecisionKind::PayPhyrexian));
    assert_eq!(g.options_desc().len(), 2, "{{R}} or 2 life");
    g.choose(Choice::Number(1));
    g.drive_until(at_priority);
    assert_eq!(g.life(0), 18);
    assert!(!g.obj(g.card(0, "Mountain", Zone::Battlefield)).tapped);

    // No red source: life only. At 1 life: mana only.
    let mut g = manual().hand(0, &["Test Phyrexian Shock"]).at(Step::Main1, 0).start();
    let s = g.card(0, "Test Phyrexian Shock", Zone::Hand);
    g.choose(Choice::Cast(s));
    g.choose(Choice::Target(Target::Player(1)));
    assert_eq!(g.options_desc(), vec!["5=1".to_string()]);
    let g = manual().hand(0, &["Test Phyrexian Shock"]).life(0, 1).at(Step::Main1, 0).start();
    let s = g.card(0, "Test Phyrexian Shock", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(s)));
}

#[test]
fn cr_107_4e_hybrid_mana_and_a_mana_choice_on_resolution() {
    // Manamorphose {1}{R/G}: "Add two mana in any combination of colors."
    let g = manual().battlefield(0, &["Island", "Plains"]).hand(0, &["Manamorphose"]).at(Step::Main1, 0).start();
    let m = g.card(0, "Manamorphose", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(m)));
    let mut g = manual().battlefield(0, &["Island", "Forest"]).hand(0, &["Manamorphose"]).at(Step::Main1, 0).start();
    let m = g.card(0, "Manamorphose", Zone::Hand);
    g.choose(Choice::Cast(m));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMana) || at_priority(g));
    g.pass();
    g.pass();
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana));
    assert_eq!(g.options_desc().len(), 15, "multisets of two colours");
    g.choose(Choice::Number(0));
    settle(&mut g);
    assert_eq!(g.player(0).pool.total(), 2);
    assert_eq!(g.player(0).hand.len(), 1, "and draws a card");
}

#[test]
fn cr_601_2b_additional_cost_options_are_a_decision() {
    // Bone Shards: sacrifice a creature or discard a card.
    let mut g = manual()
        .battlefield(0, &["Swamp", "Test Bear"])
        .hand(0, &["Bone Shards", "Island"])
        .battlefield(1, &["Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let bs = g.card(0, "Bone Shards", Zone::Hand);
    g.choose(Choice::Cast(bs));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseCost));
    assert_eq!(g.options_desc().len(), 2);
    g.choose(Choice::Number(0)); // sacrifice a creature
    let flyer = g.card(1, "Test Flyer", Zone::Battlefield);
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(g.tgt(flyer));
    g.drive_until(|g| g.kind() != Some(DecisionKind::PayMana));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice));
    let bear = g.card(0, "Test Bear", Zone::Battlefield);
    g.choose(Choice::Card(bear));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Flyer", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.count_in(0, "Island", Zone::Hand), 1);

    // Bitter Triumph: discard a card or pay 3 life; with no other card and
    // 2 life, it cannot be cast.
    let g = manual()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Bitter Triumph"])
        .battlefield(1, &["Test Flyer"])
        .life(0, 2)
        .at(Step::Main1, 0)
        .start();
    let bt = g.card(0, "Bitter Triumph", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bt)));
    let mut g = manual()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Bitter Triumph"])
        .battlefield(1, &["Test Flyer"])
        .life(0, 3)
        .at(Step::Main1, 0)
        .start();
    let bt = g.card(0, "Bitter Triumph", Zone::Hand);
    g.choose(Choice::Cast(bt));
    assert_eq!(g.options_desc(), vec!["5=1".to_string()], "only the life option");
    g.choose(Choice::Number(1));
    settle(&mut g);
    assert_eq!(g.life(0), 0);
}

#[test]
fn a_permanent_a_later_sacrifice_needs_is_not_spent_on_mana() {
    // "Sacrifice an artifact" plus {1}: Lotus Petal, the only artifact,
    // must be the sacrifice, so the Island pays.
    let mut g = manual()
        .battlefield(0, &["Lotus Petal", "Island"])
        .hand(0, &["Test Artifact Offering"])
        .at(Step::Main1, 0)
        .start();
    let o = g.card(0, "Test Artifact Offering", Zone::Hand);
    g.choose(Choice::Cast(o));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    let petal = g.card(0, "Lotus Petal", Zone::Battlefield);
    assert!(!g.has_option(act(petal, 0)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    g.choose(Choice::Card(petal));
    settle(&mut g);
    assert_eq!(g.count_in(0, "Lotus Petal", Zone::Graveyard), 1);
    // Black Lotus alone cannot be both the sacrifice and the mana (all of
    // its three mana come from one activation).
    let g = manual().battlefield(0, &["Black Lotus"]).hand(0, &["Test Artifact Offering"]).at(Step::Main1, 0).start();
    let o = g.card(0, "Test Artifact Offering", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(o)));
    // With a second artifact, the Petal may pay instead.
    let mut g = manual()
        .battlefield(0, &["Lotus Petal", "Mox Jet"])
        .hand(0, &["Test Artifact Offering"])
        .at(Step::Main1, 0)
        .start();
    let o = g.card(0, "Test Artifact Offering", Zone::Hand);
    g.choose(Choice::Cast(o));
    let petal = g.card(0, "Lotus Petal", Zone::Battlefield);
    assert!(g.has_option(act(petal, 0)));
}

#[test]
fn cr_605_3a_a_spirit_guide_is_activated_while_paying_and_revealed() {
    // Mana abilities of cards in hand are activated while paying, like any
    // other mana ability (ENGINE.md 18); not as a priority action.
    let mut g = manual()
        .battlefield(0, &["Forest", "Forest"])
        .hand(0, &["Simian Spirit Guide", "Test Double"])
        .at(Step::Main1, 0)
        .start();
    let dbl = g.card(0, "Test Double", Zone::Hand); // {2}{R}
    let guide = g.card(0, "Simian Spirit Guide", Zone::Hand);
    assert!(!g.has_option(Choice::Ability(guide, AbilitySlot::Hand(0))));
    assert!(g.has_option(Choice::Cast(dbl)), "the guide's mana counts toward castability");
    g.choose(Choice::Cast(dbl));
    assert_eq!(g.kind(), Some(DecisionKind::PayMana));
    // The red pip needs the guide; the Forests pay the generic part.
    // Sources are picked in canonical class order: Forests, then the guide.
    for _ in 0..2 {
        let forest = g.pending_decision().unwrap().options[0].choice;
        assert!(matches!(forest, Choice::Ability(c, _) if g.name(c) == "Forest"));
        g.choose(forest);
    }
    assert!(g.has_option(Choice::Ability(guide, AbilitySlot::Hand(0))));
    assert!(!g.knows(1, guide), "not revealed before it is activated");
    let keys = g.choose(Choice::Ability(guide, AbilitySlot::Hand(0)));
    // 602.2a: activating it from hand reveals it (at once, as it is picked).
    assert!(keys.iter().any(|&k| key_kind(k) == KEY_REVEAL && key_player(k) == 0));
    assert!(g.knows(1, guide));
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Simian Spirit Guide", Zone::Exile), 1);
    assert_eq!(g.stack().len(), 1);
    assert_eq!(g.player(0).pool.total(), 0);
}

#[test]
fn cr_605_3a_a_spirit_guide_pays_an_unless_cost_during_resolution() {
    // Mana Leak resolving: P0 has no priority, but may still activate mana
    // abilities to pay {3} (605.3a), the Spirit Guide's included.
    let mut g = manual()
        .battlefield(0, &["Forest", "Forest", "Mountain", "Mountain"])
        .hand(0, &["Test Bear", "Elvish Spirit Guide"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Mana Leak"])
        .at(Step::Main1, 0)
        .start();
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    while g.kind() == Some(DecisionKind::PayMana) {
        // The Forest class: both Forests.
        let forest = g
            .pending_decision()
            .unwrap()
            .options
            .iter()
            .map(|o| o.choice)
            .find(|&c| matches!(c, Choice::Ability(x, _) if g.name(x) == "Forest"))
            .unwrap();
        g.choose(forest);
    }
    g.drive_until(at_priority);
    g.pass();
    let leak = g.card(1, "Mana Leak", Zone::Hand);
    g.choose(Choice::Cast(leak));
    g.drive_until(at_priority);
    g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::YesNo), Some(0)));
    assert!(g.has_option(Choice::Yes), "two Mountains and the guide make {{3}}");
    g.choose(Choice::Yes);
    for _ in 0..2 {
        let mountain = g.pending_decision().unwrap().options[0].choice;
        assert!(matches!(mountain, Choice::Ability(c, _) if g.name(c) == "Mountain"));
        g.choose(mountain);
    }
    let guide = g.card(0, "Elvish Spirit Guide", Zone::Hand);
    assert_eq!(g.options_desc().len(), 1, "only the guide can pay the rest");
    assert!(g.has_option(Choice::Ability(guide, AbilitySlot::Hand(0))));
    g.choose(Choice::Ability(guide, AbilitySlot::Hand(0)));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Battlefield), 1);
    assert_eq!(g.count_in(0, "Elvish Spirit Guide", Zone::Exile), 1);
}

#[test]
fn a_hand_mana_source_leaves_the_cards_a_discard_cost_needs() {
    // Test Rummage: {R}, discard a card as an additional cost. The Spirit
    // Guide can pay {R} only if another card is left to discard (601.2h:
    // the mana is paid first).
    let alone = manual().hand(0, &["Test Rummage", "Simian Spirit Guide"]).at(Step::Main1, 0).start();
    let r = alone.card(0, "Test Rummage", Zone::Hand);
    assert!(!alone.has_option(Choice::Cast(r)));
    let mut g = manual().hand(0, &["Test Rummage", "Simian Spirit Guide", "Island"]).at(Step::Main1, 0).start();
    let r = g.card(0, "Test Rummage", Zone::Hand);
    g.choose(Choice::Cast(r));
    let guide = g.card(0, "Simian Spirit Guide", Zone::Hand);
    g.choose(Choice::Ability(guide, AbilitySlot::Hand(0)));
    assert_eq!(g.kind(), Some(DecisionKind::DiscardCost));
    assert_eq!(g.options_desc().len(), 1, "only the Island is left to discard");
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Island", Zone::Graveyard), 1);
}

#[test]
fn an_instant_only_mana_ability_is_activated_with_priority() {
    // Lion's Eye Diamond: discard your hand, sacrifice: three mana of one
    // colour; never while paying a cost.
    let mut g = manual()
        .battlefield(0, &["Lion's Eye Diamond"])
        .hand(0, &["Test Bear", "Island"])
        .at(Step::Upkeep, 0)
        .start();
    let led = g.card(0, "Lion's Eye Diamond", Zone::Battlefield);
    assert!(g.has_option(act(led, 0)));
    g.choose(act(led, 0));
    assert!(g.player(0).hand.is_empty());
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana));
    assert_eq!(g.options_desc().len(), 5);
    g.choose(Choice::Number(2)); // black
    assert_eq!(g.player(0).pool.amt[B], 3);
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Priority), Some(0)));
    assert!(g.stack().is_empty(), "605.3a");
    // It is not a source while paying.
    let g = manual().battlefield(0, &["Lion's Eye Diamond"]).at(Step::Main1, 0).start();
    assert!(!g.can_pay(0, &cost("{B}")));
}

#[test]
fn cycling_is_activated_from_hand() {
    let mut g = manual()
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Raffine's Tower"])
        .at(Step::Upkeep, 0)
        .start();
    let tower = g.card(0, "Raffine's Tower", Zone::Hand);
    let cyc = Choice::Ability(tower, AbilitySlot::Hand(0));
    assert!(g.has_option(cyc));
    let keys = g.choose(cyc);
    // 602.2a: activated from a hidden zone, the card is revealed.
    assert!(keys.iter().any(|&k| key_kind(k) == KEY_REVEAL));
    g.drive_until(at_priority);
    assert_eq!(g.count_in(0, "Raffine's Tower", Zone::Graveyard), 1, "discarded as a cost");
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
}

#[test]
fn cr_603_may_abilities_ask() {
    let mut g = manual().battlefield(0, &["Island", "Island"]).hand(0, &["Test May Draw"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Test May Draw", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::YesNo));
    assert_eq!(g.to_move(), Some(0));
    g.choose(Choice::Yes);
    settle(&mut g);
    assert_eq!(g.player(0).hand.len(), 1);
}

#[test]
fn cr_702_12_indestructible_and_cr_702_11_hexproof() {
    let mut g = manual()
        .battlefield(0, &["Plains", "Plains", "Plains", "Plains", "Mountain"])
        .hand(0, &["Wrath of God", "Lightning Bolt"])
        .battlefield(1, &["Test Indestructible", "Sylvan Caryatid", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    let caryatid = g.card(1, "Sylvan Caryatid", Zone::Battlefield);
    assert!(!g.has_option(g.tgt(caryatid)), "hexproof: not a target for the opponent");
    let ind = g.card(1, "Test Indestructible", Zone::Battlefield);
    g.choose(g.tgt(ind));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Indestructible", Zone::Battlefield), 1, "lethal damage does not destroy it");
    let wrath = g.card(0, "Wrath of God", Zone::Hand);
    g.choose(Choice::Cast(wrath));
    settle(&mut g);
    assert_eq!(g.count_in(1, "Test Indestructible", Zone::Battlefield), 1);
    assert_eq!(g.count_in(1, "Sylvan Caryatid", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Graveyard), 1);
}
