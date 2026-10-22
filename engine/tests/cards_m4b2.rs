//! Cube cards supported in M4b-2: hand reveals and chosen discards, modal
//! spells, tokens (library searches and looks: `rules_library.rs`).

mod common;

use common::db;
use engine::decision::act;
use engine::hidden::{key_kind, key_payload, key_player, HIDDEN, KEY_LOOK, KEY_REVEAL};
use engine::rng::Rng;
use engine::testkit::Scenario;
use engine::{Choice, DecisionKind, Game, PlayerId, Step, Zone};

fn scen() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false).library(0, &["Island"; 8]).library(1, &["Forest"; 8])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn def(name: &str) -> u16 {
    db().id(name).unwrap()
}

/// Cast from hand, answering forced decisions, until the next real one.
fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
}

/// Pass priority until something other than priority is asked or the stack
/// is empty; returns the chance keys.
fn resolve(g: &mut Game) -> Vec<u64> {
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty())
}

fn offered_names(g: &Game) -> Vec<String> {
    g.pending_decision()
        .unwrap()
        .options
        .iter()
        .map(|o| match o.choice {
            Choice::Card(c) => g.name(c).to_string(),
            Choice::Pass => "pass".to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn pick(g: &mut Game, name: &str) -> Vec<u64> {
    let c = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .map(|o| o.choice)
        .find(|&c| matches!(c, Choice::Card(x) if g.name(x) == name))
        .unwrap_or_else(|| panic!("{name} not offered: {:?}", g.options_desc()));
    g.choose(c)
}

const P1_HAND: &[&str] = &["Test Bear", "Lightning Bolt", "Counterspell", "Forest", "Time Walk"];

#[test]
fn duress_reveals_the_hand_and_the_caster_picks_a_noncreature_nonland_card() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Duress"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    // "Target opponent": P0 itself is not a legal target, so the single
    // target is chosen without a decision.
    cast(&mut g, 0, "Duress");
    let keys = resolve(&mut g);
    // Every card of the hand is revealed to everyone, in definition order.
    let mut want: Vec<u16> = P1_HAND.iter().map(|n| def(n)).collect();
    want.sort();
    let revealed: Vec<u16> = keys
        .iter()
        .filter(|&&k| key_kind(k) == KEY_REVEAL && key_player(k) == 1)
        .map(|&k| key_payload(k) as u16)
        .collect();
    assert_eq!(revealed, want);
    assert!(g.player(1).hand.iter().all(|&c| g.knows(0, c)));
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::ChooseDiscard), Some(0)));
    let mut names = offered_names(&g);
    names.sort();
    assert_eq!(names, vec!["Counterspell", "Lightning Bolt", "Time Walk"]);
    // Slots: the card's place in that hand sorted by definition.
    let hand = g.hand_sorted(1);
    for o in &g.pending_decision().unwrap().options {
        let Choice::Card(c) = o.choice else { unreachable!() };
        assert_eq!(o.action, act::LIBRARY + hand.iter().position(|&x| x == c).unwrap() as u16);
    }
    pick(&mut g, "Time Walk");
    assert_eq!(g.count_in(1, "Time Walk", Zone::Graveyard), 1);
    // The rest stays known to P0 until it changes.
    for s in 0..10 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        let mut h: Vec<&str> = d.player(1).hand.iter().map(|&c| d.name(c)).collect();
        h.sort();
        assert_eq!(h, vec!["Counterspell", "Forest", "Lightning Bolt", "Test Bear"]);
    }
    // A card P1 draws later is not known to P0.
    g.drive_until(|g| g.turn().number == 4 && g.turn().step == Step::Main1);
    let drawn = g.player(1).hand.iter().copied().find(|&c| !g.knows(0, c)).expect("a new card");
    assert_eq!(g.player(1).hand.len(), 5);
    assert!(g.knows(1, drawn));
}

#[test]
fn duress_with_nothing_to_take_only_reveals() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Duress"]).hand(1, &["Test Bear", "Forest"]).at(Step::Main1, 0).start();
    cast(&mut g, 0, "Duress");
    resolve(&mut g);
    assert!(g.stack().is_empty() && at_priority(&g));
    assert_eq!(g.player(1).hand.len(), 2);
    assert!(g.player(1).hand.iter().all(|&c| g.knows(0, c)));
}

#[test]
fn thoughtseize_and_inquisition_filter_and_cost_life() {
    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Thoughtseize"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    let ts = g.card(0, "Thoughtseize", Zone::Hand);
    g.choose(Choice::Cast(ts));
    // "Target player": either.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(Choice::Target(engine::Target::Player(1)));
    g.drive_until(at_priority);
    resolve(&mut g);
    let mut names = offered_names(&g);
    names.sort();
    assert_eq!(names, vec!["Counterspell", "Lightning Bolt", "Test Bear", "Time Walk"], "nonland");
    pick(&mut g, "Test Bear");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.life(0), 18);

    let mut g = scen().battlefield(0, &["Swamp"]).hand(0, &["Inquisition of Kozilek"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    let iok = g.card(0, "Inquisition of Kozilek", Zone::Hand);
    g.choose(Choice::Cast(iok));
    g.choose(Choice::Target(engine::Target::Player(1)));
    g.drive_until(at_priority);
    resolve(&mut g);
    let mut names = offered_names(&g);
    names.sort();
    // Mana value 3 or less: Time Walk is 2, all are.
    assert_eq!(names, vec!["Counterspell", "Lightning Bolt", "Test Bear", "Time Walk"]);
}

#[test]
fn gitaxian_probe_lets_only_its_caster_see_the_hand() {
    let mut g = scen().hand(0, &["Gitaxian Probe"]).hand(1, P1_HAND).at(Step::Main1, 0).start();
    let probe = g.card(0, "Gitaxian Probe", Zone::Hand);
    g.choose(Choice::Cast(probe));
    g.choose(Choice::Target(engine::Target::Player(1)));
    // {U/P} with no blue source: two life.
    g.drive_until(at_priority);
    assert_eq!(g.life(0), 18);
    let mut o = g.clone();
    let keys = resolve(&mut g);
    let looks: Vec<u64> = keys.iter().copied().filter(|&k| key_kind(k) == KEY_LOOK).collect();
    assert_eq!(looks.len(), 5);
    assert!(looks.iter().all(|&k| key_player(k) == 0));
    assert!(keys.iter().all(|&k| key_kind(k) != KEY_REVEAL || key_player(k) != 1), "nothing of P1's is revealed");
    assert!(g.player(1).hand.iter().all(|&c| g.knows(0, c)));
    // In P1's search the looks are keyed as hidden.
    o.determinize(1, &mut Rng::new(1));
    let keys = resolve(&mut o);
    assert!(keys.iter().filter(|&&k| key_kind(k) == KEY_LOOK).all(|&k| key_payload(k) == HIDDEN));
    // P0 drew a card off the Probe.
    assert_eq!(g.player(0).hand.len(), 1);
}

#[test]
fn a_seen_hand_partly_put_back_is_known_only_as_its_contents() {
    // P0 Probes P1, then P1 Brainstorms: P0 knows the old cards are in P1's
    // hand or the two put back, not which.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .hand(0, &["Gitaxian Probe"])
        .battlefield(1, &["Island"])
        .hand(1, &["Brainstorm", "Lightning Bolt", "Time Walk"])
        .library(0, &["Island"; 8])
        .library(1, &["Forest", "Forest", "Forest", "Plains", "Plains", "Plains", "Plains", "Plains"])
        .at(Step::Main1, 0)
        .start();
    let probe = g.card(0, "Gitaxian Probe", Zone::Hand);
    g.choose(Choice::Cast(probe));
    g.choose(Choice::Target(engine::Target::Player(1)));
    // P1's turn: Brainstorm.
    g.drive_until(|g| g.turn().number == 4 && g.turn().step == Step::Main1);
    let bs = g.card(1, "Brainstorm", Zone::Hand);
    g.choose(Choice::Cast(bs));
    g.drive_until(|g| g.kind() == Some(DecisionKind::ChooseHandCard));
    pick(&mut g, "Time Walk");
    pick(&mut g, "Forest");
    g.check_invariants().unwrap();
    let k = g.clouds().iter().find(|k| k.viewer == 0).expect("P0's partial knowledge");
    let mut want = vec![def("Lightning Bolt"), def("Time Walk")];
    want.sort();
    assert_eq!(k.defs, want);
    let tw = g.card(1, "Time Walk", Zone::Library);
    let (mut lib, mut hand) = (0, 0);
    for s in 0..100 {
        let mut d = g.clone();
        d.determinize(0, &mut Rng::new(s));
        d.check_invariants().unwrap();
        let top2: Vec<_> = d.player(1).library.iter().rev().take(2).copied().collect();
        let tw_top = top2.iter().any(|&c| d.obj(c).def == def("Time Walk"));
        let tw_hand = d.player(1).hand.iter().any(|&c| d.obj(c).def == def("Time Walk"));
        assert!(tw_top || tw_hand, "seed {s}");
        lib += tw_top as u32;
        hand += tw_hand as u32;
    }
    assert!(lib > 10 && hand > 10, "top {lib}, hand {hand}");
    assert!(!g.knows(0, tw));
}

// ---- modal spells (CR 700.2) ------------------------------------------------

fn modes_offered(g: &Game) -> Vec<u8> {
    let d = g.pending_decision().unwrap();
    assert_eq!(d.kind, DecisionKind::ChooseMode, "{:?}", g.options_desc());
    d.options
        .iter()
        .filter_map(|o| match o.choice {
            Choice::Number(k) => Some(k),
            _ => None,
        })
        .collect()
}

#[test]
fn cr_700_2a_modes_are_chosen_on_casting_before_targets() {
    // Abrade: 3 damage to target creature, or destroy target artifact.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain"])
        .hand(0, &["Abrade"])
        .battlefield(1, &["Test Bear", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let abrade = g.card(0, "Abrade", Zone::Hand);
    g.choose(Choice::Cast(abrade));
    assert_eq!(modes_offered(&g), vec![0, 1]);
    assert!(!g.has_option(Choice::Pass), "choose one: exactly one");
    g.choose(Choice::Number(1));
    // 700.2d-ish: only the chosen mode's targets: artifacts.
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    let ring = g.card(1, "Sol Ring", Zone::Battlefield);
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    assert!(g.has_option(g.tgt(ring)) && !g.has_option(g.tgt(bear)));
    g.choose(g.tgt(ring));
    g.drive_until(at_priority);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Graveyard), 1);
    assert_eq!(g.obj(bear).damage, 0);
}

#[test]
fn cr_700_2_a_mode_without_legal_targets_cannot_be_chosen() {
    let mut g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &["Abrade"]).battlefield(1, &["Test Bear"]).at(Step::Main1, 0).start();
    let abrade = g.card(0, "Abrade", Zone::Hand);
    g.choose(Choice::Cast(abrade));
    assert_eq!(modes_offered(&g), vec![0], "no artifact to target");
    // With nothing to target at all, it cannot be cast.
    let g = scen().battlefield(0, &["Mountain", "Mountain"]).hand(0, &["Abrade"]).at(Step::Main1, 0).start();
    let abrade = g.card(0, "Abrade", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(abrade)));
}

#[test]
fn fiery_confluence_repeats_modes_in_non_decreasing_order() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 4])
        .battlefield(0, &["Test Bear"])
        .hand(0, &["Fiery Confluence"])
        .battlefield(1, &["Test Reacher", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    assert_eq!(modes_offered(&g), vec![0, 1, 2]);
    g.choose(Choice::Number(1));
    // Non-decreasing: mode 0 is gone, mode 1 may repeat.
    assert_eq!(modes_offered(&g), vec![1, 2]);
    g.choose(Choice::Number(1));
    assert!(!g.has_option(Choice::Pass), "choose three");
    g.choose(Choice::Number(2));
    // The third mode's target, then the payment.
    let ring = g.card(1, "Sol Ring", Zone::Battlefield);
    g.choose(g.tgt(ring));
    g.drive_until(at_priority);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.life(1), 16, "2 damage to each opponent, twice");
    assert_eq!(g.count_in(1, "Sol Ring", Zone::Graveyard), 1);
    let reacher = g.card(1, "Test Reacher", Zone::Battlefield);
    assert_eq!(g.obj(reacher).damage, 0, "mode 0 not chosen");

    // Mode 0 three times: 3 damage to each creature, both sides.
    let mut g = scen()
        .battlefield(0, &["Mountain"; 4])
        .battlefield(0, &["Test Bear"])
        .hand(0, &["Fiery Confluence"])
        .battlefield(1, &["Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let fc = g.card(0, "Fiery Confluence", Zone::Hand);
    g.choose(Choice::Cast(fc));
    for _ in 0..3 {
        g.choose(Choice::Number(0));
    }
    g.drive_until(at_priority);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Test Reacher", Zone::Graveyard), 1);
}

#[test]
fn mystic_confluence_gives_each_mode_instance_its_own_target() {
    // Return target creature twice: two instances, each with its own
    // target (the same creature may be chosen for both, CR 115.3); then
    // draw.
    let mut g = scen()
        .battlefield(0, &["Island"; 5])
        .hand(0, &["Mystic Confluence"])
        .battlefield(1, &["Test Bear", "Test Reacher"])
        .at(Step::Main1, 0)
        .start();
    let mc = g.card(0, "Mystic Confluence", Zone::Hand);
    g.choose(Choice::Cast(mc));
    assert_eq!(modes_offered(&g), vec![1, 2], "no spell to counter");
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(2));
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    let reacher = g.card(1, "Test Reacher", Zone::Battlefield);
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    g.choose(g.tgt(bear));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    assert!(g.has_option(g.tgt(bear)), "another instance may target it again");
    g.choose(g.tgt(reacher));
    g.drive_until(at_priority);
    let hand = g.player(0).hand.len();
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Hand), 1);
    assert_eq!(g.count_in(1, "Test Reacher", Zone::Hand), 1);
    assert_eq!(g.player(0).hand.len(), hand + 1, "drew one");
}

#[test]
fn cr_608_2b_a_modal_spell_with_every_target_illegal_does_not_resolve() {
    // Mystic Confluence: bounce the Bear twice and draw. P1 exiles its own
    // Bear in response: every target is illegal, so even the draw does not
    // happen.
    let mut g = scen()
        .battlefield(0, &["Island"; 5])
        .hand(0, &["Mystic Confluence"])
        .battlefield(1, &["Test Bear", "Plains"])
        .hand(1, &["Swords to Plowshares"])
        .at(Step::Main1, 0)
        .start();
    let mc = g.card(0, "Mystic Confluence", Zone::Hand);
    g.choose(Choice::Cast(mc));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(1));
    g.choose(Choice::Number(2));
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    g.choose(g.tgt(bear));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    g.pass();
    let stp = g.card(1, "Swords to Plowshares", Zone::Hand);
    g.choose(Choice::Cast(stp));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    let hand = g.player(0).hand.len();
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(1, "Test Bear", Zone::Exile), 1);
    assert_eq!(g.player(0).hand.len(), hand, "the draw mode did not happen");
    assert_eq!(g.count_in(0, "Mystic Confluence", Zone::Graveyard), 1);
}

#[test]
fn pyroblast_targets_any_spell_and_counters_only_a_blue_one() {
    // A red spell: Pyroblast can target it but does nothing.
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Lightning Bolt"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Pyroblast"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(Choice::Target(engine::Target::Player(1)));
    g.drive_until(at_priority);
    g.pass();
    let pb = g.card(1, "Pyroblast", Zone::Hand);
    g.choose(Choice::Cast(pb));
    assert_eq!(modes_offered(&g), vec![0, 1], "a spell and permanents to target");
    g.choose(Choice::Number(0));
    g.drive_until(at_priority);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.life(1), 17, "the Bolt resolved");
    // A blue spell is countered.
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Test Flyer"])
        .battlefield(1, &["Mountain"])
        .hand(1, &["Pyroblast"])
        .at(Step::Main1, 0)
        .start();
    let flyer = g.card(0, "Test Flyer", Zone::Hand);
    g.choose(Choice::Cast(flyer));
    g.drive_until(at_priority);
    g.pass();
    let pb = g.card(1, "Pyroblast", Zone::Hand);
    g.choose(Choice::Cast(pb));
    g.choose(Choice::Number(0));
    g.drive_until(at_priority);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(0, "Test Flyer", Zone::Graveyard), 1);
}

#[test]
fn cr_603_3c_a_modal_trigger_chooses_its_mode_as_it_goes_on_the_stack() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Test Modal Entry"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Test Modal Entry", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty() && g.waiting_triggers().is_empty());
    assert_eq!(modes_offered(&g), vec![0, 1]);
    g.choose(Choice::Number(1));
    assert_eq!(g.kind(), Some(DecisionKind::Target));
    let bear = g.card(1, "Test Bear", Zone::Battlefield);
    g.choose(g.tgt(bear));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.obj(bear).damage, 1);
    assert_eq!(g.life(0), 20);
}

// ---- tokens (CR 111) ----------------------------------------------------------

#[test]
fn cr_111_a_token_is_a_new_object_and_ceases_to_exist_off_the_battlefield() {
    // Thraben Inspector investigates: a Clue token ({2}, sacrifice: draw).
    let mut g = scen().battlefield(0, &["Plains", "Island", "Island"]).hand(0, &["Thraben Inspector"]).at(Step::Main1, 0).start();
    let n = g.objs().len();
    cast(&mut g, 0, "Thraben Inspector");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g) && g.waiting_triggers().is_empty());
    assert_eq!(g.objs().len(), n + 1, "one new object slot");
    let clue = g.card(0, "Clue", Zone::Battlefield);
    assert!(g.def_of(clue).token && g.is_type(clue, engine::db::types::ARTIFACT));
    assert_eq!(g.obj(clue).owner, 0);
    g.check_invariants().unwrap();
    // Sacrifice it to draw: it goes to the graveyard (where "dies"-style
    // triggers would see it), then ceases to exist (704.5d).
    let hand = g.player(0).hand.len();
    g.choose(Choice::Ability(clue, engine::db::AbilitySlot::Activated(0)));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.player(0).hand.len(), hand + 1);
    assert_eq!(g.obj(clue).zone, Zone::Nowhere);
    assert!(g.player(0).graveyard.iter().all(|&c| c != clue));
    g.check_invariants().unwrap();
}

#[test]
fn cr_704_5d_a_token_put_into_a_library_ceases_to_exist() {
    // Oust puts the Golem token into its owner's library: gone at the next
    // state-based action check; its controller still gains 3 life.
    let mut g = scen()
        .battlefield(0, &["Mountain", "Mountain", "Mountain", "Sol Ring"])
        .battlefield(0, &["Legion Extruder"])
        .battlefield(1, &["Plains"])
        .hand(1, &["Oust"])
        .at(Step::Main1, 0)
        .start();
    let ext = g.card(0, "Legion Extruder", Zone::Battlefield);
    g.choose(Choice::Ability(ext, engine::db::AbilitySlot::Activated(0)));
    // Sacrifice another artifact: Sol Ring (after paying {2} with Mountains).
    while g.kind() != Some(DecisionKind::Sacrifice) {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
    let ring = g.card(0, "Sol Ring", Zone::Battlefield);
    g.choose(Choice::Card(ring));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let golem = g.card(0, "Golem", Zone::Battlefield);
    assert_eq!((g.power(golem), g.toughness(golem)), (3, 3));
    g.drive_until(|g| g.turn().number == 4 && g.turn().step == Step::Main1);
    let oust = g.card(1, "Oust", Zone::Hand);
    g.choose(Choice::Cast(oust));
    g.choose(g.tgt(golem));
    g.drive_until(at_priority);
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.obj(golem).zone, Zone::Nowhere);
    assert_eq!(g.life(0), 23);
    assert!(!g.player(0).library.contains(&golem));
    g.check_invariants().unwrap();
    for s in 0..10 {
        let mut d = g.clone();
        d.determinize(1, &mut Rng::new(s));
        d.check_invariants().unwrap();
    }
}

#[test]
fn malevolent_rumble_reveals_four_takes_a_permanent_and_makes_a_spawn() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Forest", "Forest"])
        .hand(0, &["Malevolent Rumble", "Test Double"])
        .library(0, &["Lightning Bolt", "Test Bear", "Forest", "Counterspell", "Island", "Island"])
        .library(1, &["Island"; 6])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Malevolent Rumble");
    let keys = resolve(&mut g);
    let shown: Vec<u16> = keys.iter().filter(|&&k| key_kind(k) == KEY_REVEAL).map(|&k| key_payload(k) as u16).collect();
    assert_eq!(shown, ["Lightning Bolt", "Test Bear", "Forest", "Counterspell"].map(def).to_vec(), "revealed, top first");
    assert_eq!(g.kind(), Some(DecisionKind::PutInHand));
    let mut names = offered_names(&g);
    names.sort();
    assert_eq!(names, vec!["Forest", "Test Bear", "pass"], "permanent cards; may take none");
    pick(&mut g, "Test Bear");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(0, "Test Bear", Zone::Hand), 1);
    for n in ["Lightning Bolt", "Forest", "Counterspell"] {
        assert_eq!(g.count_in(0, n, Zone::Graveyard), 1, "{n}: the rest into the graveyard");
    }
    let spawn = g.card(0, "Eldrazi Spawn", Zone::Battlefield);
    assert_eq!((g.power(spawn), g.toughness(spawn)), (0, 1));
    // "Sacrifice this token: Add {C}" pays for Test Double ({2}{R})... with
    // no red source it cannot; Test Bear ({1}{G}): Forest + Spawn after
    // untapping. Here: the Spawn is a mana source while paying.
    g.drive_until(|g| g.turn().number == 5 && g.turn().step == Step::Main1);
    let bear = g.card(0, "Test Bear", Zone::Hand);
    g.choose(Choice::Cast(bear));
    // Sources in canonical class order: a Forest, then the Spawn may pay
    // the generic part.
    let forest = g.pending_decision().unwrap().options[0].choice;
    g.choose(forest);
    let spawn_opt = g
        .pending_decision()
        .unwrap()
        .options
        .iter()
        .any(|o| matches!(o.choice, Choice::Ability(c, _) if g.name(c) == "Eldrazi Spawn"));
    assert!(spawn_opt, "{:?}", g.options_desc());
}

#[test]
fn thopter_foundry_sacrifices_only_nontoken_artifacts() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island", "Thopter Foundry", "Mind Stone"])
        .at(Step::Main1, 0)
        .start();
    let tf = g.card(0, "Thopter Foundry", Zone::Battlefield);
    g.choose(Choice::Ability(tf, engine::db::AbilitySlot::Activated(0)));
    while g.kind() != Some(DecisionKind::Sacrifice) {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
    let stone = g.card(0, "Mind Stone", Zone::Battlefield);
    g.choose(Choice::Card(stone));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    let thopter = g.card(0, "Thopter", Zone::Battlefield);
    assert!(g.has_kw(thopter, engine::db::Kw::Flying));
    assert_eq!(g.def_of(thopter).colors, 1 << engine::mana::U, "a blue token (colour indicator)");
    assert_eq!(g.life(0), 21);
    // Now only the Foundry itself is a nontoken artifact: it can still be
    // sacrificed to itself, but the Thopter cannot.
    g.choose(Choice::Ability(tf, engine::db::AbilitySlot::Activated(0)));
    while g.kind() != Some(DecisionKind::Sacrifice) {
        let first = g.pending_decision().unwrap().options[0].choice;
        g.choose(first);
    }
    assert!(!g.has_option(Choice::Card(thopter)));
    assert!(g.has_option(Choice::Card(tf)));
}

#[test]
fn sheoldreds_edict_makes_each_opponent_sacrifice_their_choice() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Swamp"])
        .hand(0, &["Sheoldred's Edict"])
        .battlefield(1, &["Test Bear", "Test Flyer"])
        .at(Step::Main1, 0)
        .start();
    let se = g.card(0, "Sheoldred's Edict", Zone::Hand);
    g.choose(Choice::Cast(se));
    assert_eq!(modes_offered(&g), vec![0, 1, 2], "no targets: every mode");
    g.choose(Choice::Number(0));
    g.drive_until(at_priority);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    // The opponent chooses which nontoken creature.
    assert_eq!((g.kind(), g.to_move()), (Some(DecisionKind::Sacrifice), Some(1)));
    let flyer = g.card(1, "Test Flyer", Zone::Battlefield);
    g.choose(Choice::Card(flyer));
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    assert_eq!(g.count_in(1, "Test Flyer", Zone::Graveyard), 1);
    assert_eq!(g.count_in(1, "Test Bear", Zone::Battlefield), 1);
}

#[test]
fn generous_ent_forestcycles_or_makes_food() {
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Mountain"])
        .hand(0, &["Generous Ent"])
        .library(0, &["Breeding Pool", "Island", "Forest"])
        .library(1, &["Island"; 6])
        .at(Step::Main1, 0)
        .start();
    let ent = g.card(0, "Generous Ent", Zone::Hand);
    g.choose(Choice::Ability(ent, engine::db::AbilitySlot::Hand(0)));
    g.drive_until(at_priority);
    resolve(&mut g);
    let mut d = g.pending_decision().unwrap().options.iter().filter_map(|o| match o.choice {
        Choice::Def(x) => Some(x),
        _ => None,
    }).collect::<Vec<_>>();
    d.sort();
    let mut want = vec![def("Breeding Pool"), def("Forest")];
    want.sort();
    assert_eq!(d, want, "Forest cards: Breeding Pool is a Forest");
}

#[test]
fn timetwister_shuffles_hands_and_graveyards_away_and_forgets_their_places() {
    // P1 knew the Time Walk in P0's hand. After Timetwister it is in P0's
    // library or new hand: P1 knows nothing more about it.
    let mut g = Scenario::new(db())
        .spec(|s| s.auto_single = false)
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Timetwister", "Forest"])
        .hand_known(0, &["Time Walk"])
        .graveyard(0, &["Lightning Bolt"])
        .graveyard(1, &["Counterspell", "Plains"])
        .hand(1, &["Mountain"])
        .library(0, &["Island"; 10])
        .library(1, &["Forest"; 10])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Timetwister");
    g.drive_until(|g| g.stack().is_empty() && at_priority(g));
    for p in 0..2 {
        assert_eq!(g.player(p).hand.len(), 7);
    }
    // Only Timetwister itself is in a graveyard.
    assert!(g.player(1).graveyard.is_empty());
    assert_eq!(g.player(0).graveyard.len(), 1);
    assert_eq!(g.player(0).library.len(), 10 + 2 + 1 - 7);
    assert_eq!(g.player(1).library.len(), 10 + 1 + 2 - 7);
    g.check_invariants().unwrap();
    assert!(g.clouds().is_empty(), "{:?}", g.clouds());
    assert!(g.player(0).hand.iter().all(|&c| !g.knows(1, c)));
}

#[test]
fn cr_700_2b_a_trigger_with_no_mode_chosen_is_removed() {
    let mut g = scen().battlefield(0, &["Plains", "Plains"]).hand(0, &["Test Up To One"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Test Up To One", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(at_priority);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty() && g.waiting_triggers().is_empty());
    assert_eq!(modes_offered(&g), vec![0]);
    assert!(g.has_option(Choice::Pass), "up to one");
    g.pass();
    assert!(g.stack().is_empty(), "removed from the stack");
    assert_eq!(g.life(0), 20);
}
