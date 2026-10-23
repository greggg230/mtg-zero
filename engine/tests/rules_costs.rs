//! M7b part 3b (ENGINE.md 27.6): the total cost of a spell (CR 601.2f) and
//! "cards" in a graveyard (a token isn't a card, 108.2, 111.1).

mod common;

use common::db;
use engine::db::Cond;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, Zone};

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

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
    panic!("resolve_with: no progress\n{}", g.log_lines().join("\n"));
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().unwrap().options.iter().map(|o| o.choice).collect()
}

/// Green Goblin ("spells you cast from your graveyard cost {2} less";
/// "each nonland card in your graveyard has mayhem") out, a Mountain for
/// Faithless Looting, which draws the two cards on top of the library and
/// discards them (mayhem: discarded this turn).
fn looted(top: &[&str], lands: &[&str], opp: &[&str]) -> Game {
    let mut lib: Vec<&str> = top.to_vec();
    lib.extend(["Island"; 6]);
    let mut field = vec!["Norman Osborn // Green Goblin", "Mountain"];
    field.extend(lands);
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &lib)
        .library(1, &["Forest"; 10])
        .battlefield(0, &field)
        .transformed(0, "Norman Osborn // Green Goblin")
        .battlefield(1, opp)
        .hand(0, &["Faithless Looting"])
        .start();
    // (The top cards by position: `library` lists the top last.)
    let lib0 = &g.player(0).library;
    let wanted: Vec<CardIdx> = lib0[lib0.len() - top.len()..].to_vec();
    for (c, n) in wanted.iter().rev().zip(top) {
        assert_eq!(g.name(*c), *n);
    }
    let loot = g.card(0, "Faithless Looting", Zone::Hand);
    let m = g.card(0, "Mountain", Zone::Battlefield);
    g.choose(Choice::Cast(loot));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Discard) => options(g).into_iter().find(|c| matches!(c, Choice::Card(x) if wanted.contains(x))),
        Some(DecisionKind::PayMana) => options(g).into_iter().find(|c| matches!(c, Choice::Ability(x, _) if *x == m)),
        _ => None,
    });
    for c in wanted {
        assert_eq!(g.obj(c).zone, Zone::Graveyard);
    }
    g
}

#[test]
fn kicker_is_part_of_the_total_cost_before_reductions_cr_601_2f() {
    // Tear Asunder ({1}{G}, kicker {1}{B}) cast with mayhem under Green
    // Goblin, kicked: {1}{G} + {1}{B} - {2} = {G}{B}. Unkicked it has no
    // target (no artifact or enchantment), so it is castable only kicked,
    // with a Forest and a Swamp.
    let mut g = looted(&["Tear Asunder", "Island"], &["Forest", "Swamp"], &["Test Bear"]);
    let tear = g.card(0, "Tear Asunder", Zone::Graveyard);
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    assert!(g.has_option(Choice::Cast(tear)), "kicked for {{G}}{{B}}: {:?}", g.options_desc());
    g.choose(Choice::Cast(tear));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseCost));
    assert_eq!(options(&g), vec![Choice::Number(1)], "only kicked");
    g.choose(Choice::Number(1));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert!(g.lands_of(0).all(|c| g.obj(c).tapped), "{{G}}{{B}} and the Looting's {{R}}");
}

#[test]
fn x_is_part_of_the_mana_cost_before_reductions_cr_601_2f() {
    // Walking Ballista ({X}{X}) cast with mayhem under Green Goblin: X = 2
    // costs {4} - {2} = {2}, payable with two Islands.
    let mut g = looted(&["Walking Ballista", "Island"], &["Island", "Island"], &[]);
    let wb = g.card(0, "Walking Ballista", Zone::Graveyard);
    g.choose(Choice::Cast(wb));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseMode));
    assert_eq!(options(&g), vec![Choice::Number(0), Choice::Number(1), Choice::Number(2)]);
    g.choose(Choice::Number(2));
    resolve_with(&mut g, |_| None);
    let wb = g.card(0, "Walking Ballista", Zone::Battlefield);
    assert_eq!(g.pt(wb), (2, 2));
}

#[test]
fn a_cost_reduction_applies_to_an_alternative_cost_cr_601_2f_118_9d() {
    // "Costs {1} less for each artifact you control. You may pay {2}{U}
    // rather than pay this spell's mana cost": with one artifact, the
    // alternative cost is {1}{U}; the mana cost, {2}{U}, can't be paid with
    // two Islands.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .library(0, &["Island"; 5])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Island", "Island", "Sensei's Divining Top"])
        .hand(0, &["Test Discount Alt"])
        .start();
    let c = g.card(0, "Test Discount Alt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "{:?}", g.options_desc());
    let hand = g.player(0).hand.len();
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseAltCost));
    assert_eq!(options(&g), vec![Choice::Number(1)], "only the alternative cost");
    g.choose(Choice::Number(1));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).hand.len(), hand, "cast it (-1), drew a card (+1)");
    assert_eq!(g.lands_of(0).filter(|&c| g.obj(c).tapped).count(), 2);
}

#[test]
fn a_token_in_a_graveyard_is_not_a_card_cr_108_2() {
    // Between a token dying and 704.5d it is in a graveyard, but not as a
    // card: card types among cards in graveyards (the Lhurgoyfs), delirium
    // and threshold ignore it.
    let mut g = Scenario::new(db())
        .library(0, &["Island"; 5])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Barrowgoyf", "Dragon's Rage Channeler"])
        .graveyard(0, &["Forest", "Lightning Bolt", "Ponder", "Island", "Island", "Island"])
        .start();
    let goyf = g.card(0, "Barrowgoyf", Zone::Battlefield);
    let drc = g.card(0, "Dragon's Rage Channeler", Zone::Battlefield);
    // Land, instant, sorcery: 3/4 and no delirium.
    assert_eq!(g.pt(goyf), (3, 4));
    assert_eq!(g.pt(drc), (1, 1));
    assert!(g.holds(&Cond::GraveyardAtLeast(6), 0) && !g.holds(&Cond::GraveyardAtLeast(7), 0));
    let token = g.put_token_in_graveyard(0, "Spirit");
    assert!(g.is_token(token) && g.obj(token).zone == Zone::Graveyard);
    assert_eq!(g.pt(goyf), (3, 4), "a creature token isn't a creature card");
    assert_eq!(g.pt(drc), (1, 1), "no delirium");
    assert!(!g.holds(&Cond::Delirium(4), 0));
    assert!(!g.holds(&Cond::GraveyardAtLeast(7), 0), "no threshold from a token");
}
