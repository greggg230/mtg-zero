//! Costs that read the graveyard, paid whole (CR 601.2h; ENGINE.md 247): the fuzz finds of the farm's payment lane,
//! each as a scenario. Every option a cast offers must lead to a completed cast, whatever is chosen after it: the tests
//! walk every decision of the cast every way.

use engine::db::CardDb;
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, Step, Zone};

fn cube() -> Scenario {
    Scenario::new(CardDb::cube()).spec(|s| s.auto_single = false).library(0, &["Island"; 12]).library(1, &["Island"; 12])
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

/// Every state at the next priority from `g`'s pending decision, each decision answered every way (at most `limit`
/// states; a decision with no option panics in the engine).
fn every_way(g: &Game, limit: usize) -> Vec<Game> {
    let mut stack = vec![g.clone()];
    let mut out = Vec::new();
    while let Some(x) = stack.pop() {
        if out.len() >= limit {
            break;
        }
        if x.is_over() || at_priority(&x) {
            out.push(x);
            continue;
        }
        for c in x.offered() {
            let mut y = x.clone();
            y.choose(c);
            stack.push(y);
        }
    }
    out
}

/// CR 601.2h, 702.138a (ruling bea302d9: a card the cost itself puts into the graveyard may be exiled for it): Tinker
/// ("as an additional cost to cast this spell, sacrifice an artifact") escaped by Underworld Breach with two other
/// cards in the graveyard and a Lotus Petal the only artifact - the Petal sacrificed for Tinker is the third card. The
/// payment activated the Petal's own mana ability first "so that it is in the graveyard" (`exile_sources`, which
/// counted the cost's discards but not its sacrifices), leaving Tinker no artifact: "picks within the budget" (fuzz
/// seed 100061712, ENGINE.md 247.2).
#[test]
fn cr_601_2h_702_138a_the_costs_own_sacrifice_feeds_the_escape_not_a_mana_source_first() {
    let g = cube()
        .battlefield(0, &["Underworld Breach", "Lotus Petal", "Island", "Island", "Island"])
        .graveyard(0, &["Tinker", "Ponder", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let tinker = g.card(0, "Tinker", Zone::Graveyard);
    let petal = g.card(0, "Lotus Petal", Zone::Battlefield);
    assert!(g.has_option(Choice::Cast(tinker)), "{}", opts(&g));
    let mut h = g.clone();
    h.choose(Choice::Cast(tinker));
    let ends = every_way(&h, 500);
    assert!(!ends.is_empty());
    for e in &ends {
        assert_eq!(e.obj(tinker).zone, Zone::Stack, "{}", log(e));
        assert_eq!(e.obj(petal).zone, Zone::Exile, "the Petal sacrificed for Tinker, then exiled by escape: {}", log(e));
    }
}

/// CR 702.66a, 702.138a, 601.2g-h: Dig Through Time ({6}{U}{U}, delve) escaped by Underworld Breach with seven other
/// cards in the graveyard and a Black Lotus the only mana source. Three cards are the escape's, so delve has four and
/// the Lotus (sacrificed for {U}{U}{U}, then delved): {1}{U}{U} left. Delve's sources are chosen before its graveyard
/// cards, and four graveyard cards picked first left {2}{U}{U} with three mana: "PayMana: no way to pay" (fuzz seed
/// 100353550, ENGINE.md 247.5). A graveyard card is offered only while the graveyard can give every pick still needed
/// besides the escape's cards.
#[test]
fn cr_702_66a_702_138a_delves_graveyard_picks_leave_the_escapes_cards() {
    let g = cube()
        .battlefield(0, &["Underworld Breach", "Black Lotus"])
        .graveyard(0, &["Dig Through Time", "Ponder", "Brainstorm", "Preordain", "Lightning Bolt", "Counterspell", "Mountain", "Forest"])
        .at(Step::Main1, 0)
        .start();
    let dig = g.card(0, "Dig Through Time", Zone::Graveyard);
    let lotus = g.card(0, "Black Lotus", Zone::Battlefield);
    assert!(g.has_option(Choice::Cast(dig)), "{}", opts(&g));
    let mut h = g.clone();
    h.choose(Choice::Cast(dig));
    let ends = every_way(&h, 3000);
    assert!(!ends.is_empty());
    for e in &ends {
        assert_eq!(e.obj(dig).zone, Zone::Stack, "{}", log(e));
        assert_ne!(e.obj(lotus).zone, Zone::Battlefield, "the Lotus pays: {}", log(e));
    }
}

/// CR 601.2b, 601.2g-h, 702.138a: Bitter Triumph ("as an additional cost to cast this spell, discard a card or pay 3
/// life") escaped by Underworld Breach with one other card in the graveyard: the card discarded and a Chromatic Star
/// sacrificed for mana while it is cast (601.2g) are the second and third. Castability counted that (the 601.2g
/// step's search, ENGINE.md 231.4); the choice of the additional cost asked the payment alone and offered nothing:
/// "ChooseCost with no options" (fuzz seed 100438064 with hidden decklists, ENGINE.md 247.3). It offers the discard
/// (not the life: two cards only).
#[test]
fn cr_601_2b_702_138a_the_additional_cost_choice_counts_the_steps_feeding_abilities() {
    let g = cube()
        .battlefield(0, &["Underworld Breach", "Chromatic Star", "Swamp", "Swamp", "Swamp"])
        .battlefield(1, &["Llanowar Elves"])
        .hand(0, &["Island"])
        .graveyard(0, &["Bitter Triumph", "Ponder"])
        .at(Step::Main1, 0)
        .start();
    let bt = g.card(0, "Bitter Triumph", Zone::Graveyard);
    let star = g.card(0, "Chromatic Star", Zone::Battlefield);
    assert!(g.has_option(Choice::Cast(bt)), "{}", opts(&g));
    let mut h = g.clone();
    h.choose(Choice::Cast(bt));
    assert_eq!(h.kind(), Some(DecisionKind::ChooseCost), "{}\n{}", opts(&h), log(&h));
    assert_eq!(h.offered(), vec![Choice::Number(0)], "the discard: {}", opts(&h));
    let ends = every_way(&h, 2000);
    assert!(!ends.is_empty());
    for e in &ends {
        assert_eq!(e.obj(bt).zone, Zone::Stack, "{}", log(e));
        assert_eq!(e.obj(star).zone, Zone::Exile, "the Star sacrificed while casting, then exiled by escape: {}", log(e));
    }
}

/// CR 702.81a, 712.11b (ENGINE.md 10, 247.4): Six gives Norman Osborn // Green Goblin in P0's graveyard retrace; P0
/// begins to cast it and is asked which face. The faces offered were found castable counting the land card in P0's
/// hand, so that card is pinned in every world dealt for P1 while the cast is in progress - as it is at "which
/// graveyard card?" before and once the spell is on the stack. At the face choice it wasn't: a world gave P0 a hand
/// with no land and the cast had nothing to discard, "DiscardCost with no options" (fuzz seed 100654376).
#[test]
fn cr_702_81a_retraces_land_card_is_pinned_at_the_face_choice() {
    let mut g = cube()
        .library(0, &["Lightning Bolt"; 12])
        .battlefield(0, &["Six", "Underground Sea", "Badlands", "Volcanic Island", "Watery Grave"])
        .hand(0, &["Island", "Lightning Bolt", "Lightning Bolt"])
        .graveyard(0, &["Norman Osborn // Green Goblin"])
        .at(Step::Main1, 0)
        .start();
    let norman = g.card(0, "Norman Osborn // Green Goblin", Zone::Graveyard);
    let island = g.card(0, "Island", Zone::Hand);
    for _ in 0..4 {
        if g.kind() == Some(DecisionKind::ChooseFace) {
            break;
        }
        let c = g
            .offered()
            .into_iter()
            .find(|c| matches!(c, Choice::Cast(x) | Choice::CastFace(x) | Choice::Faces(x, _) | Choice::Card(x) if *x == norman))
            .unwrap_or_else(|| panic!("no cast of the card: {}", opts(&g)));
        g.choose(c);
    }
    assert_eq!(g.kind(), Some(DecisionKind::ChooseFace), "{}\n{}", opts(&g), log(&g));
    assert!(g.pinned_for(1).contains(&island), "the land card the retrace counted on: {:?}", g.pinned_for(1));
    for s in 0..40 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        assert_eq!(d.obj(island).zone, Zone::Hand);
        assert_eq!(d.name(island), "Island", "world {s}");
        for e in every_way(&d, 200) {
            assert_eq!(e.obj(norman).zone, Zone::Stack, "world {s}: {}", log(&e));
        }
    }
}
