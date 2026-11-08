//! Card tests of the Edge of Eternities cards W6 finishes (ENGINE.md 99.16),
//! each ability against its Oracle text and the official rulings.

mod eoe_b;

use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_b::*;

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

/// P1 casts `spell` for alternative cost `alt` (0: its mana cost) with X =
/// `x`; P0 gets priority with it on the stack.
fn p1_casts_with(g: &mut Game, spell: &str, alt: u8, x: u8) {
    let c = g.card(1, spell, Zone::Hand);
    start_with(g, Choice::Cast(c), |g| match g.kind() {
        Some(DecisionKind::ChooseAltCost) => Some(Choice::Number(alt)),
        Some(DecisionKind::ChooseMode) if g.has_option(Choice::Number(x)) => Some(Choice::Number(x)),
        _ => None,
    });
    assert_eq!(g.stack().len(), 1, "{spell} on the stack\n{}", log(g));
    g.pass();
    assert!(at_priority(g) && g.pending_decision().unwrap().player == 0, "P0's priority\n{}", log(g));
}

// ---- Unravel (docs/ir_defects.md 44) -----------------------------------------

/// Unravel: "Counter target spell. If the amount of mana spent to cast that
/// spell was less than its mana value, you draw a card." Broodguard Elite
/// ({X}{G}{G}, warp {X}{G}) cast for its mana cost with X = 1 spent 3, its
/// mana value 3 (X counts on the stack: CR 202.3e, Unravel's ruling of
/// 2025-07-25): no card. Cast for warp with X = 1 it spent 2: a card (an
/// alternative cost doesn't change the mana value, CR 118.9c, the other
/// ruling). Germinating Wurm warped ({1}{G} for a 5-mana spell): a card.
#[test]
fn cr_601_2h_unravel_draws_when_less_mana_was_spent_than_the_mana_value() {
    for (spell, alt, x, draws) in [("Broodguard Elite", 0u8, 1u8, false), ("Broodguard Elite", 1, 1, true), ("Germinating Wurm", 1, 0, true), ("Germinating Wurm", 0, 0, false)] {
        let mut g = scen()
            .battlefield(0, &["Island", "Island", "Island"])
            .hand(0, &["Unravel"])
            .battlefield(1, &["Forest"; 5])
            .hand(1, &[spell])
            .at(Step::Main1, 1)
            .start();
        p1_casts_with(&mut g, spell, alt, x);
        let hand = g.player(0).hand.len();
        let u = g.card(0, "Unravel", Zone::Hand);
        start_with(&mut g, Choice::Cast(u), |_| None);
        settle(&mut g);
        assert!(g.find_card(1, spell, Zone::Graveyard).is_some(), "{spell} countered\n{}", log(&g));
        // (Unravel left the hand; a card drawn makes up for it.)
        assert_eq!(g.player(0).hand.len(), hand - 1 + draws as usize, "{spell} alt {alt} x {x}\n{}", log(&g));
    }
}
