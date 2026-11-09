//! Card tests of W6's counter cards (ENGINE.md 99.16): Loading Zone's
//! counter-doubling replacement (CR 614.1a, 122.6), against its Oracle text
//! and its rulings of 2025-07-25.

mod eoe_a_kit;

use engine::db::Counter;
use engine::{Choice, DecisionKind, Game, Step, Zone};
use eoe_a_kit::*;

fn cast_on(g: &mut Game, p: u8, name: &str, target: Option<engine::CardIdx>) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| g.kind() != Some(DecisionKind::Priority) || !g.stack().is_empty());
    if let Some(t) = target {
        if g.kind() == Some(DecisionKind::Target) {
            g.choose(g.tgt(t));
        }
    }
    pay(g);
    settle(g);
}

/// Loading Zone: "If one or more counters would be put on a creature,
/// Spacecraft, or Planet you control, twice that many of each of those kinds
/// of counters are put on it instead." Honor's +1/+1 counter on a creature
/// you control: two; on an opponent's: one. Two Loading Zones: four (the
/// ruling: "four times the original number").
#[test]
fn cr_614_1a_loading_zone_doubles_counters_on_your_creatures() {
    for (zones, mine, want) in [(1usize, true, 2u64), (1, false, 1), (2, true, 4), (0, true, 1)] {
        let mut bf0: Vec<&str> = vec!["Plains", "Lane A Bear"];
        bf0.extend(std::iter::repeat_n("Loading Zone", zones));
        let mut g = scen().battlefield(0, &bf0).battlefield(1, &["Lane A Bear"]).hand(0, &["Honor"]).at(Step::Main1, 0).start();
        let bear = bf(&g, if mine { 0 } else { 1 }, "Lane A Bear");
        cast_on(&mut g, 0, "Honor", Some(bear));
        assert_eq!(g.counters_on(bear, Counter::P1P1), want, "zones {zones} mine {mine}\n{}", log(&g));
    }
}

/// "If a creature, Spacecraft, or Planet you control would enter the
/// battlefield with a number of counters on it, it enters with twice that
/// many instead" (ruling): Dockworker Drone ("enters with a +1/+1 counter")
/// enters with two.
#[test]
fn cr_122_6_loading_zone_doubles_counters_a_creature_enters_with() {
    let mut g = scen().battlefield(0, &["Plains", "Plains", "Loading Zone"]).hand(0, &["Dockworker Drone"]).at(Step::Main1, 0).start();
    cast_on(&mut g, 0, "Dockworker Drone", None);
    let d = bf(&g, 0, "Dockworker Drone");
    assert_eq!(g.counters_on(d, Counter::P1P1), 2, "{}", log(&g));
}

/// Every kind of counter, on a Spacecraft (a noncreature artifact below its
/// threshold) and on a Planet (a land): station's charge counters (CR
/// 702.184a) are doubled; a noncreature artifact that is neither gets none
/// more (Lane A Trinket: no counters are put on it here, so the Spacecraft
/// stands for the recipients' filter).
#[test]
fn cr_702_184a_loading_zone_doubles_charge_counters_on_spacecraft_and_planets() {
    let mut g = scen()
        .battlefield(0, &["Wurmwall Sweeper", "Lane A Planet", "Lane A Bear", "Lane A Giant", "Loading Zone"])
        .at(Step::Main1, 0)
        .start();
    let ship = bf(&g, 0, "Wurmwall Sweeper");
    let planet = bf(&g, 0, "Lane A Planet");
    let (bear, giant) = (bf(&g, 0, "Lane A Bear"), bf(&g, 0, "Lane A Giant"));
    station(&mut g, ship, bear);
    assert_eq!(g.counters_on(ship, charge()), 4, "2 doubled\n{}", log(&g));
    station(&mut g, planet, giant);
    assert_eq!(g.counters_on(planet, charge()), 10, "5 doubled\n{}", log(&g));
}

/// The priority option that activates `c`'s (non-mana) activated ability.
fn activation(g: &Game, c: engine::CardIdx) -> Choice {
    g.pending_decision()
        .unwrap()
        .options
        .iter()
        .map(|o| o.choice)
        .find(|ch| matches!(ch, Choice::Ability(x, engine::db::AbilitySlot::Activated(_)) if *x == c))
        .unwrap_or_else(|| panic!("{} can't be activated: {:?}", g.name(c), g.options_desc()))
}

/// Activate Nesting Grounds with targets `a` then `b`; `kind` answers the
/// counter-kind choice (None: no choice expected).
fn nest(g: &mut Game, a: engine::CardIdx, b: engine::CardIdx, kind: Option<u8>) {
    let ng = bf(g, 0, "Nesting Grounds");
    let ch = activation(g, ng);
    g.choose(ch);
    let mut asked = false;
    let mut n_t = 0;
    for _ in 0..50 {
        if at_priority(g) && g.stack().is_empty() {
            break;
        }
        if at_priority(g) {
            g.pass();
            continue;
        }
        match g.kind() {
            Some(DecisionKind::Target) => {
                let t = if n_t == 0 { a } else { b };
                n_t += 1;
                g.choose(g.tgt(t));
            }
            Some(DecisionKind::ChooseMode) => {
                asked = true;
                g.choose(Choice::Number(kind.expect("no kind choice expected")));
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    assert_eq!(asked, kind.is_some(), "the kind choice\n{}", log(g));
}

// ---- Nesting Grounds (docs/ir_defects.md 27) ---------------------------------

/// Nesting Grounds: "{1}, {T}: Move a counter from target permanent you
/// control onto a second target permanent. Activate only as a sorcery." A
/// +1/+1 counter from P0's creature onto P1's (the second target may be
/// anyone's, CR 122.5: removed from the first, put on the second).
#[test]
fn cr_122_5_nesting_grounds_moves_a_counter_onto_any_second_permanent() {
    let mut g = scen()
        .battlefield(0, &["Nesting Grounds", "Plains", "Lane A Bear"])
        .battlefield(1, &["Lane A Giant"])
        .counters(0, "Lane A Bear", Counter::P1P1, 2)
        .at(Step::Main1, 0)
        .start();
    let (bear, giant) = (bf(&g, 0, "Lane A Bear"), bf(&g, 1, "Lane A Giant"));
    nest(&mut g, bear, giant, None);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "{}", log(&g));
    assert_eq!(g.counters_on(giant, Counter::P1P1), 1);
}

/// "You choose which kind of counter to move as that ability resolves"
/// (ruling of 2020-04-17): a Spacecraft with charge counters and a +1/+1
/// counter; the charge counter goes onto a creature ("loyalty counters on
/// creatures or +1/+1 counters on lands", the ruling: any permanent can get
/// any kind).
#[test]
fn cr_122_5_nesting_grounds_moves_the_kind_chosen_on_resolution() {
    let mut g = scen()
        .battlefield(0, &["Nesting Grounds", "Plains", "Wurmwall Sweeper", "Lane A Bear"])
        .counters(0, "Wurmwall Sweeper", charge(), 2)
        .counters(0, "Wurmwall Sweeper", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let (ship, bear) = (bf(&g, 0, "Wurmwall Sweeper"), bf(&g, 0, "Lane A Bear"));
    // Kinds in kind order: +1/+1 (0), charge (1).
    nest(&mut g, ship, bear, Some(1));
    assert_eq!(g.counters_on(ship, charge()), 1, "{}", log(&g));
    assert_eq!(g.counters_on(ship, Counter::P1P1), 1);
    assert_eq!(g.counters_on(bear, charge()), 1);
}

/// The first target is a permanent you control, the second another one
/// (CR 115.3: "a second target"); with nothing to move, nothing happens
/// (CR 122.5). Moving onto a creature you control with Loading Zone puts two
/// (the ruling: abilities that care about counters being put apply).
#[test]
fn cr_115_3_nesting_grounds_targets_and_loading_zone() {
    let mut g = scen()
        .battlefield(0, &["Nesting Grounds", "Plains", "Lane A Bear", "Lane A Giant", "Loading Zone"])
        .battlefield(1, &["Lane A Robot"])
        .counters(0, "Lane A Bear", Counter::P1P1, 1)
        .at(Step::Main1, 0)
        .start();
    let (bear, giant, robot) = (bf(&g, 0, "Lane A Bear"), bf(&g, 0, "Lane A Giant"), bf(&g, 1, "Lane A Robot"));
    let ng = bf(&g, 0, "Nesting Grounds");
    let ch = activation(&g, ng);
    g.choose(ch);
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)) && !g.has_option(g.tgt(robot)), "the first target is yours: {:?}", g.options_desc());
    g.choose(g.tgt(bear));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    assert!(!g.has_option(g.tgt(bear)) && g.has_option(g.tgt(robot)), "a second permanent, anyone's: {:?}", g.options_desc());
    g.choose(g.tgt(giant));
    while !at_priority(&g) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
    }
    settle(&mut g);
    // (The setup's counter on the Bear was doubled too: two, one moved.)
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "{}", log(&g));
    assert_eq!(g.counters_on(giant, Counter::P1P1), 2, "Loading Zone doubles the counter put");
}

// ---- Terrasymbiosis (docs/ir_defects.md 31) ----------------------------------

/// Activate Intrepid Tenderfoot's "{3}: Put a +1/+1 counter on this
/// creature" and resolve it; the may-draw is answered `draw`.
fn grow(g: &mut Game, draw: bool) {
    let t = bf(g, 0, "Intrepid Tenderfoot");
    let ch = activation(g, t);
    g.choose(ch);
    resolve_with(g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => Some(if draw { Choice::Yes } else { Choice::Pass }),
        _ => None,
    });
}

/// Terrasymbiosis: "Whenever you put one or more +1/+1 counters on a
/// creature you control, you may draw that many cards. Do this only once
/// each turn." Declined the first time, it still triggers (CR 603.2h: only
/// once the action was taken); drawn, it doesn't trigger again that turn;
/// the next turn it does. With Loading Zone, "that many" is the two put.
#[test]
fn cr_603_2h_terrasymbiosis_draws_that_many_once_each_turn() {
    let mut g = scen()
        .battlefield(0, &["Terrasymbiosis", "Intrepid Tenderfoot"])
        .battlefield(0, &["Forest"; 9])
        .at(Step::Main1, 0)
        .start();
    let hand = g.player(0).hand.len();
    grow(&mut g, false);
    assert_eq!(g.player(0).hand.len(), hand, "declined\n{}", log(&g));
    grow(&mut g, true);
    assert_eq!(g.player(0).hand.len(), hand + 1, "drew 1\n{}", log(&g));
    let waiting_before = g.log_lines().len();
    grow(&mut g, true);
    assert_eq!(g.player(0).hand.len(), hand + 1, "done this turn: no trigger\n{}", log(&g));
    let _ = waiting_before;
    // Next turn, with Loading Zone: two counters, two cards.
    let mut g = scen()
        .battlefield(0, &["Terrasymbiosis", "Intrepid Tenderfoot", "Loading Zone"])
        .battlefield(0, &["Forest"; 3])
        .at(Step::Main1, 0)
        .start();
    let hand = g.player(0).hand.len();
    grow(&mut g, true);
    let t = bf(&g, 0, "Intrepid Tenderfoot");
    assert_eq!(g.counters_on(t, Counter::P1P1), 2);
    assert_eq!(g.player(0).hand.len(), hand + 2, "that many: two\n{}", log(&g));
}

/// "Whenever you put": counters an opponent's ability puts on your creature
/// don't trigger it (CR 122.6); counters your creature enters with, you put
/// (CR 122.6a: its controller), so they do (Dockworker Drone).
#[test]
fn cr_122_6a_terrasymbiosis_only_for_counters_you_put() {
    let mut g = scen()
        .battlefield(0, &["Terrasymbiosis", "Lane A Bear"])
        .battlefield(1, &["Sunset Saboteur"])
        .at(Step::Main1, 1)
        .start();
    let bear = bf(&g, 0, "Lane A Bear");
    let sab = bf(&g, 1, "Sunset Saboteur");
    let hand = g.player(0).hand.len();
    // P1 attacks with Sunset Saboteur ("Whenever this creature attacks, put
    // a +1/+1 counter on target creature an opponent controls").
    for _ in 0..200 {
        if g.counters_on(bear, Counter::P1P1) > 0 || g.turn().active == 0 {
            break;
        }
        let d = g.pending_decision().unwrap();
        let c = match d.kind {
            DecisionKind::DeclareAttackers if g.has_option(Choice::Card(sab)) => Choice::Card(sab),
            DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers | DecisionKind::Priority => Choice::Pass,
            DecisionKind::Target => g.tgt(bear),
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    settle(&mut g);
    assert_eq!(g.counters_on(bear, Counter::P1P1), 1, "{}", log(&g));
    assert_eq!(g.player(0).hand.len(), hand, "P1 put it: no trigger\n{}", log(&g));
    assert!(!g.pending_decision().is_some_and(|d| d.kind == DecisionKind::YesNo && d.player == 0));
    let mut g = scen().battlefield(0, &["Terrasymbiosis", "Plains", "Plains"]).hand(0, &["Dockworker Drone"]).at(Step::Main1, 0).start();
    let hand = g.player(0).hand.len();
    let d = g.card(0, "Dockworker Drone", Zone::Hand);
    g.choose(Choice::Cast(d));
    pay(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::YesNo)).then_some(Choice::Yes));
    assert_eq!(g.player(0).hand.len(), hand - 1 + 1, "entered with a counter you put: drew 1\n{}", log(&g));
}
