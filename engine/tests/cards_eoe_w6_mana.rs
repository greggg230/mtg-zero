//! W6's mana and land pieces (ENGINE.md 99.16): modular (CR 702.43, Power
//! Depot's) on custom test cards.

mod eoe_w6_kit;

use engine::db::Counter;
use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_w6_kit::*;

/// P0 destroys its own artifact `c` with Shattered Wings; the modular
/// trigger targets `to` and its "may" is answered `yes`.
fn destroy(g: &mut Game, c: engine::CardIdx, to: engine::CardIdx, yes: bool) {
    let w = g.card(0, "Shattered Wings", Zone::Hand);
    start_with(g, Choice::Cast(w), |g| pick(g, &[g.tgt(c)]));
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::Target) => pick(g, &[g.tgt(to)]),
        Some(DecisionKind::YesNo) => Some(if yes { Choice::Yes } else { Choice::Pass }),
        _ => None,
    });
}

/// Modular (CR 702.43a): "This permanent enters with two +1/+1 counters on
/// it. When it is put into a graveyard from the battlefield, you may put a
/// +1/+1 counter on target artifact creature for each +1/+1 counter on it"
/// (as it last existed, 608.2h). Declined: nothing.
#[test]
fn cr_702_43a_modular_enters_with_counters_and_passes_them_on() {
    for yes in [true, false] {
        let mut g = scen()
            .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Forest", "Dockworker Drone"])
            .hand(0, &["W6 Modular Bot", "Shattered Wings"])
            .at(Step::Main1, 0)
            .start();
        let bot = g.card(0, "W6 Modular Bot", Zone::Hand);
        start_with(&mut g, Choice::Cast(bot), |_| None);
        settle(&mut g);
        assert_eq!(g.counters_on(bot, Counter::P1P1), 2, "{}", log(&g));
        let drone = bf(&g, 0, "Dockworker Drone");
        let before = g.counters_on(drone, Counter::P1P1);
        destroy(&mut g, bot, drone, yes);
        assert_eq!(g.obj(bot).zone, Zone::Graveyard);
        assert_eq!(g.counters_on(drone, Counter::P1P1), before + if yes { 2 } else { 0 }, "{}", log(&g));
    }
}

/// "When it's put into the graveyard from the battlefield, you may put its
/// +1/+1 counters onto target artifact creature, no matter what Power
/// Depot's card types are at the time" (Power Depot's ruling): a
/// noncreature artifact with modular 1 enters with a counter (it does
/// nothing there) and passes it on.
#[test]
fn cr_702_43a_modular_on_a_noncreature_artifact() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Dockworker Drone"])
        .hand(0, &["W6 Modular Relic", "Shattered Wings"])
        .at(Step::Main1, 0)
        .start();
    let relic = g.card(0, "W6 Modular Relic", Zone::Hand);
    start_with(&mut g, Choice::Cast(relic), |_| None);
    settle(&mut g);
    assert_eq!(g.counters_on(relic, Counter::P1P1), 1);
    let drone = bf(&g, 0, "Dockworker Drone");
    let before = g.counters_on(drone, Counter::P1P1);
    destroy(&mut g, relic, drone, true);
    assert_eq!(g.counters_on(drone, Counter::P1P1), before + 1, "{}", log(&g));
}
