//! Workstream B of the faithfulness audit (docs/faithfulness_audit.md 5.3,
//! 5.4; ENGINE.md 100.4): mana and payment, exact.

mod eoe_w2;

use eoe_w2::*;
use engine::Step;

/// Gene Pollinator ("{T}, Tap an untapped permanent you control: Add one
/// mana of any color") with many lands: every land may be tapped for its own
/// ability or for a Pollinator's (CR 601.2g, 605.3a), and the payment's
/// feasibility is decided exactly, whatever the number of lands (EoE fuzz
/// seed 10023: two Pollinators and 13 lands made each check a search of 3^13
/// leaves, made thousands of times per decision; with 20 lands one check
/// passed the search's node limit, a loud failure; ENGINE.md 100.4).
#[test]
fn cr_601_2g_two_gene_pollinators_and_twenty_lands_are_decided_exactly() {
    let lands = ["Island", "Swamp", "Mountain", "Forest"].repeat(5);
    let mut bfs: Vec<&str> = vec!["Gene Pollinator", "Gene Pollinator"];
    bfs.extend(lands.iter().copied());
    let g = scen().battlefield(0, &bfs).at(Step::Main1, 0).start();
    // Each Pollinator taps a land for {W}: two {W}, eighteen lands left.
    assert!(g.can_pay(0, &cost("{W}{W}{18}")));
    // One more is too much: a third {W} needs a third activation, and a
    // Pollinator tapping the other makes one {W} where two lands made two.
    assert!(!g.can_pay(0, &cost("{W}{W}{19}")));
    assert!(!g.can_pay(0, &cost("{W}{W}{W}")));
    // Twenty-one: one Pollinator taps the other, every land for itself.
    assert!(g.can_pay(0, &cost("{W}{20}")));
    assert!(!g.can_pay(0, &cost("{22}")));
}
