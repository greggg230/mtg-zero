//! Floating mana from a source about to be lost (ENGINE.md 86; docs/shortcuts.md
//! 13.12). Each test names the CR rule it checks.
//!
//! * With priority, the mana abilities of an untapped mana source are offered
//!   while an object on the stack targets it (CR 605.3a, 117.1d): tap a
//!   Volcanic Island for {R} in response to Wasteland, cast Lightning Bolt
//!   after Wasteland resolves, in the same step.
//! * A cost that returns or sacrifices an untapped mana source (and doesn't tap
//!   it) offers its mana ability first (CR 601.2g before 601.2h): tap the
//!   Island for {U}, return it for Daze, cast Brainstorm with the {U}.
//! * The mana stays in the pool until the step ends (CR 106.4, 500.4).

use engine::db::{AbilitySlot, CardDb};
use engine::mana;
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn scen() -> Scenario {
    Scenario::new(CardDb::constructed())
        .spec(|s| {
            s.auto_single = false;
            s.log = true;
        })
        .library(0, &["Island"; 10])
        .library(1, &["Island"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn act(c: CardIdx, i: u8) -> Choice {
    Choice::Ability(c, AbilitySlot::Activated(i))
}

fn log(g: &Game) -> String {
    g.log_lines().join("\n")
}

fn opts(g: &Game) -> String {
    g.options_desc().join(" | ")
}

fn options(g: &Game) -> Vec<Choice> {
    g.pending_decision().map(|d| d.options.iter().map(|o| o.choice).collect()).unwrap_or_default()
}

fn decider(g: &Game) -> PlayerId {
    g.pending_decision().unwrap().player
}

fn pool(g: &Game, p: PlayerId) -> [u8; 6] {
    g.player(p).pool.amt
}

/// A mana ability of a source that is never offered: any `Ability` option
/// of the pending decision naming a mana ability.
fn mana_options(g: &Game) -> Vec<Choice> {
    options(g)
        .into_iter()
        .filter(|c| match *c {
            Choice::Ability(x, slot) => g.def_of(x).activated_def(slot).mana().is_some(),
            _ => false,
        })
        .collect()
}

/// Answer a `ChooseMana` decision with the option that adds mana of type `t`
/// to `p`'s pool.
fn choose_mana(g: &mut Game, p: PlayerId, t: usize) {
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana), "{}", opts(g));
    let before = pool(g, p)[t];
    for c in options(g) {
        let mut h = g.clone();
        h.choose(c);
        if pool(&h, p)[t] > before {
            g.choose(c);
            return;
        }
    }
    panic!("no option adds that mana: {}", opts(g));
}

/// Run the stack down (priority passed; anything else answered with its
/// first option) until it is empty and someone has priority.
fn settle(g: &mut Game) {
    for _ in 0..300 {
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
            return;
        }
        if at_priority(g) {
            g.pass();
        } else {
            let c = options(g)[0];
            g.choose(c);
        }
    }
    panic!("settle: no progress\n{}", log(g));
}

/// P1's turn (main phase 1): P1 activates Wasteland at P0's Volcanic Island,
/// then passes; P0 has priority with the ability on the stack.
fn wasteland_at_volcanic(extra0: &[&str]) -> (Game, CardIdx) {
    let mut bf0 = vec!["Volcanic Island"];
    bf0.extend_from_slice(extra0);
    let mut g = scen()
        .battlefield(1, &["Wasteland"])
        .battlefield(0, &bf0)
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let waste = bf(&g, 1, "Wasteland");
    let volc = bf(&g, 0, "Volcanic Island");
    assert!(at_priority(&g) && decider(&g) == 1);
    // "{T}, Sacrifice Wasteland: Destroy target nonbasic land."
    g.choose(act(waste, 1));
    assert_eq!(g.kind(), Some(DecisionKind::Target), "{}", opts(&g));
    g.choose(g.tgt(volc));
    g.drive_until(at_priority);
    assert_eq!(decider(&g), 1, "the activator receives priority (CR 117.3c)");
    g.pass();
    assert!(at_priority(&g) && decider(&g) == 0);
    (g, volc)
}

#[test]
fn cr_605_3a_float_in_response_to_wasteland_then_bolt_after_it_resolves() {
    let (mut g, volc) = wasteland_at_volcanic(&[]);
    // The targeted untapped source's mana ability, with priority.
    assert!(g.has_option(act(volc, 0)), "{}", opts(&g));
    g.choose(act(volc, 0));
    choose_mana(&mut g, 0, mana::R);
    assert!(at_priority(&g) && decider(&g) == 0, "a mana ability leaves priority where it is (CR 605.3a)");
    assert_eq!(pool(&g, 0), [0, 0, 0, 1, 0, 0]);
    assert!(g.obj(volc).tapped);
    assert!(mana_options(&g).is_empty(), "tapped: no second activation: {}", opts(&g));
    // Both pass: Wasteland resolves, P1 (the active player) gets priority.
    g.pass();
    g.pass();
    assert_eq!(g.obj(volc).zone, Zone::Graveyard, "{}", log(&g));
    assert!(g.stack().is_empty());
    assert_eq!(g.turn().step, Step::Main1);
    assert_eq!(pool(&g, 0)[mana::R], 1, "the {{R}} is still in the pool (CR 106.4)");
    g.pass();
    assert_eq!(decider(&g), 0);
    // Lightning Bolt with the floated {R}: no land left.
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bolt)), "{}", opts(&g));
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 0), [0; 6], "the {{R}} paid for it");
    settle(&mut g);
    assert_eq!(g.life(1), 17, "{}", log(&g));
    assert_eq!(g.turn().step, Step::Main1, "the same step");
}

#[test]
fn cr_500_4_floated_mana_empties_at_the_end_of_the_step() {
    let (mut g, volc) = wasteland_at_volcanic(&[]);
    g.choose(act(volc, 0));
    choose_mana(&mut g, 0, mana::R);
    g.pass();
    g.pass();
    assert_eq!(pool(&g, 0)[mana::R], 1);
    g.pass_until(|g| g.turn().step != Step::Main1);
    assert_ne!(g.turn().step, Step::Main1);
    assert_eq!(pool(&g, 0), [0; 6], "mana empties from the pool at the end of each step (CR 500.4, 106.4)");
    // (In the next step the Bolt has nothing to pay with.)
    g.pass_until(|g| decider(g) == 0);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(bolt)), "{}", opts(&g));
}

#[test]
fn cr_605_3a_no_float_option_when_the_source_is_tapped() {
    // P0 taps the targeted Volcanic Island for a Lightning Bolt first: no
    // mana ability is offered for it afterwards.
    let (mut g, volc) = wasteland_at_volcanic(&[]);
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    assert!(g.obj(volc).tapped);
    assert_eq!(decider(&g), 0);
    assert!(mana_options(&g).is_empty(), "{}", opts(&g));
}

#[test]
fn cr_605_3a_no_float_option_when_the_source_is_not_targeted() {
    // Wasteland at the Thundering Falls: the Falls' mana ability is offered,
    // the untapped Volcanic Island's is not (nothing targets it).
    let mut g = scen()
        .battlefield(1, &["Wasteland"])
        .battlefield(0, &["Volcanic Island", "Thundering Falls"])
        .hand(0, &["Lightning Bolt"])
        .at(Step::Main1, 1)
        .start();
    let waste = bf(&g, 1, "Wasteland");
    let volc = bf(&g, 0, "Volcanic Island");
    let falls = bf(&g, 0, "Thundering Falls");
    assert!(!g.obj(falls).tapped && !g.obj(volc).tapped);
    g.choose(act(waste, 1));
    g.choose(g.tgt(falls));
    g.drive_until(at_priority);
    g.pass();
    assert_eq!(decider(&g), 0);
    assert_eq!(mana_options(&g), vec![act(falls, 0)], "{}", opts(&g));
    // With an empty stack nothing is offered at all (ENGINE.md 18).
    let g = scen().battlefield(0, &["Volcanic Island", "Thundering Falls", "Wasteland"]).at(Step::Main1, 0).start();
    assert!(at_priority(&g));
    assert!(mana_options(&g).is_empty(), "{}", opts(&g));
}

#[test]
fn cr_601_2g_daze_tap_the_island_for_mana_then_return_it_and_cast_brainstorm() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"])
        .hand(1, &["Daze", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    g.pass();
    assert_eq!(decider(&g), 1);
    let island = bf(&g, 1, "Island");
    let daze = g.card(1, "Daze", Zone::Hand);
    g.choose(Choice::Cast(daze));
    // (Its mana cost is unpayable: the alternative cost; the Bolt as its
    // target: both forced.)
    g.drive_until(|g| g.kind() == Some(DecisionKind::BounceCost));
    assert_eq!(g.kind(), Some(DecisionKind::BounceCost), "{}", opts(&g));
    // Return the Island, or tap it for {U} first (CR 601.2g before 601.2h).
    assert_eq!(options(&g), vec![Choice::Card(island), act(island, 0)], "{}", opts(&g));
    g.choose(act(island, 0));
    g.drive_until(at_priority);
    assert_eq!(decider(&g), 1, "{}", log(&g));
    assert_eq!(g.obj(island).zone, Zone::Hand, "returned as the cost");
    assert_eq!(pool(&g, 1), [0, 1, 0, 0, 0, 0], "and its {{U}} floats");
    assert_eq!(g.stack().len(), 2);
    // Brainstorm with the floated {U}, in response.
    let bs = g.card(1, "Brainstorm", Zone::Hand);
    assert!(g.has_option(Choice::Cast(bs)), "{}", opts(&g));
    g.choose(Choice::Cast(bs));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 1), [0; 6]);
    assert_eq!(g.stack().len(), 3);
    settle(&mut g);
    assert_eq!(g.obj(bs).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(g.obj(bolt).zone, Zone::Graveyard, "countered: P0's Mountain is tapped, no {{1}}");
    assert_eq!(g.life(1), 20);
    assert_eq!(g.player(1).hand.len(), 2, "the Island, three drawn, two put back");
    assert_eq!(g.turn().step, Step::Main1);
}

#[test]
fn cr_601_2g_daze_a_volcanic_island_floats_either_colour() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Volcanic Island"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    g.pass();
    let volc = bf(&g, 1, "Volcanic Island");
    g.choose(Choice::Cast(g.card(1, "Daze", Zone::Hand)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::BounceCost));
    assert_eq!(options(&g), vec![Choice::Card(volc), act(volc, 0)], "{}", opts(&g));
    g.choose(act(volc, 0));
    assert_eq!(g.kind(), Some(DecisionKind::ChooseMana), "{}", opts(&g));
    assert_eq!(options(&g).len(), 2, "{{U}} or {{R}}");
    choose_mana(&mut g, 1, mana::R);
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 1), [0, 0, 0, 1, 0, 0]);
    assert_eq!(g.obj(volc).zone, Zone::Hand);
}

#[test]
fn cr_601_2g_no_float_when_the_island_is_tapped() {
    // P1 taps its Island for a Brainstorm first, then Dazes the Bolt
    // returning that Island: nothing to float.
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"])
        .hand(1, &["Daze", "Brainstorm"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    g.pass();
    let island = bf(&g, 1, "Island");
    g.choose(Choice::Cast(g.card(1, "Brainstorm", Zone::Hand)));
    g.drive_until(at_priority);
    assert!(g.obj(island).tapped);
    g.choose(Choice::Cast(g.card(1, "Daze", Zone::Hand)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) && g.pending_decision().unwrap().options.len() > 1);
    // Daze's target: the Bolt (not the Brainstorm).
    let at_bolt = options(&g).into_iter().zip(g.options_desc()).find(|(_, d)| d.contains("Lightning Bolt")).unwrap().0;
    g.choose(at_bolt);
    g.drive_until(|g| g.kind() == Some(DecisionKind::BounceCost) || at_priority(g));
    assert_eq!(g.kind(), Some(DecisionKind::BounceCost), "{}", opts(&g));
    assert_eq!(options(&g), vec![Choice::Card(island)], "tapped: only the return");
    g.choose(Choice::Card(island));
    g.drive_until(at_priority);
    assert_eq!(pool(&g, 1), [0; 6]);
    assert_eq!(g.obj(island).zone, Zone::Hand);
}

#[test]
fn cr_601_2g_two_untapped_islands_each_may_float() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island", "Island"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    g.choose(Choice::Cast(g.card(0, "Lightning Bolt", Zone::Hand)));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    g.pass();
    let daze = g.card(1, "Daze", Zone::Hand);
    g.choose(Choice::Cast(daze));
    // Two Islands pay {1}{U}: the mana cost or the alternative cost.
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(1));
    }
    g.drive_until(|g| g.kind() == Some(DecisionKind::BounceCost));
    let islands: Vec<CardIdx> = g.permanents_of(1).collect();
    let want: Vec<Choice> = vec![Choice::Card(islands[0]), Choice::Card(islands[1]), act(islands[0], 0), act(islands[1], 0)];
    let mut got = options(&g);
    got.sort_by_key(|c| format!("{c:?}"));
    let mut want = want;
    want.sort_by_key(|c| format!("{c:?}"));
    assert_eq!(got, want, "{}", opts(&g));
}

/// The spectator view's words for the pending options (the Play tab's labels).
fn labels(g: &Game) -> Vec<String> {
    let v = g.spectator_view();
    v["decision"]["options"].as_array().unwrap().iter().map(|o| o["t"].as_str().unwrap().to_string()).collect()
}

#[test]
fn the_spectator_view_names_the_float_options() {
    let (mut g, volc) = wasteland_at_volcanic(&[]);
    assert!(labels(&g).contains(&"Tap Volcanic Island for mana".to_string()), "{:?}", labels(&g));
    g.choose(act(volc, 0));
    assert_eq!(labels(&g), vec!["Add {U}", "Add {R}"]);
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Island"])
        .hand(1, &["Daze"])
        .at(Step::Main1, 0)
        .start();
    g.choose(Choice::Cast(g.card(0, "Lightning Bolt", Zone::Hand)));
    g.choose(Choice::Target(Target::Player(1)));
    g.drive_until(at_priority);
    g.pass();
    g.choose(Choice::Cast(g.card(1, "Daze", Zone::Hand)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::BounceCost));
    assert_eq!(labels(&g), vec!["Return Island to hand", "Tap Island for mana, then return it to hand"]);
}

#[test]
fn cr_601_2g_tinker_taps_the_sol_ring_it_sacrifices() {
    // Tinker's mana comes from the pool; its additional cost sacrifices an
    // artifact: the untapped Sol Ring may make {C}{C} first (CR 601.2g-h).
    let mut g = scen().battlefield(0, &["Sol Ring"]).hand(0, &["Tinker"]).pool(0, [0, 1, 0, 0, 0, 2], 0).at(Step::Main1, 0).start();
    let ring = bf(&g, 0, "Sol Ring");
    g.choose(Choice::Cast(g.card(0, "Tinker", Zone::Hand)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Sacrifice));
    assert_eq!(g.kind(), Some(DecisionKind::Sacrifice), "{}", opts(&g));
    assert_eq!(pool(&g, 0), [0; 6], "the pool paid {{2}}{{U}}");
    assert_eq!(options(&g), vec![Choice::Card(ring), act(ring, 0)], "{}", opts(&g));
    g.choose(act(ring, 0));
    g.drive_until(at_priority);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard, "{}", log(&g));
    assert_eq!(pool(&g, 0), [0, 0, 0, 0, 0, 2], "Sol Ring's {{C}}{{C}} floats");
}

#[test]
fn fetch_lands_and_wasteland_activate_as_before() {
    // Their costs tap the source themselves ({T}, sacrifice): no float
    // option while they are paid, and no mana ability at an empty stack.
    let mut g = scen()
        .battlefield(0, &["Scalding Tarn", "Wasteland", "Volcanic Island"])
        .battlefield(1, &["Volcanic Island"])
        .at(Step::Main1, 0)
        .start();
    let tarn = bf(&g, 0, "Scalding Tarn");
    let waste = bf(&g, 0, "Wasteland");
    let theirs = bf(&g, 1, "Volcanic Island");
    assert!(mana_options(&g).is_empty(), "{}", opts(&g));
    // The fetch land: {T}, pay 1 life, sacrifice: no decision before priority.
    g.choose(act(tarn, 0));
    assert!(at_priority(&g), "{}", opts(&g));
    assert_eq!(g.obj(tarn).zone, Zone::Graveyard);
    assert!(mana_options(&g).is_empty(), "nothing of P0's is targeted: {}", opts(&g));
    settle(&mut g);
    // Wasteland at P1's Volcanic Island: a target, then priority.
    g.choose(act(waste, 1));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(mana_options(&g).is_empty());
    g.choose(g.tgt(theirs));
    assert!(at_priority(&g) && decider(&g) == 0, "{}", opts(&g));
    assert_eq!(g.obj(waste).zone, Zone::Graveyard);
    assert!(mana_options(&g).is_empty(), "P0's own land isn't targeted: {}", opts(&g));
    // P1, whose Volcanic Island is targeted, may float its mana.
    g.pass();
    assert_eq!(decider(&g), 1);
    assert_eq!(mana_options(&g), vec![act(theirs, 0)], "{}", opts(&g));
}
