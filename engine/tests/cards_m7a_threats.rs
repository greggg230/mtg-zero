//! M7a: damage dealt by an event's object, tokens entering attacking (CR
//! 508.4), reflexive triggers (603.12), random bottom order, graveyard
//! exile costs, "the second time this ability resolved this turn".

mod common;

use common::db;
use engine::db::{AbilitySlot, Counter, Kw};
use engine::testkit::Scenario;
use engine::{CardIdx, Choice, DecisionKind, Game, PlayerId, Step, Target, Zone};

fn base() -> Scenario {
    Scenario::new(db()).spec(|s| s.auto_single = false)
}

fn scen() -> Scenario {
    base().library(0, &["Island"; 10]).library(1, &["Forest"; 10])
}

fn at_priority(g: &Game) -> bool {
    g.kind() == Some(DecisionKind::Priority)
}

fn bf(g: &Game, p: PlayerId, name: &str) -> CardIdx {
    g.card(p, name, Zone::Battlefield)
}

fn pay(g: &mut Game) {
    while g.kind() == Some(DecisionKind::PayMana) {
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        g.drive_until(at_priority);
    }
}

fn cast(g: &mut Game, p: PlayerId, name: &str) {
    let c = g.card(p, name, Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    g.drive_until(at_priority);
    pay(g);
}

/// Run the stack down; `answer` answers what is asked (None: the first
/// option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..200 {
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
    panic!("resolve_with: no progress");
}

/// Pass priority, decline attacks and blocks, answer the rest with the
/// first option until `stop`.
fn quiet_until(g: &mut Game, stop: impl Fn(&Game) -> bool) {
    for _ in 0..10_000 {
        if stop(g) {
            return;
        }
        let d = g.pending_decision().expect("game running");
        let c = match d.kind {
            DecisionKind::Priority | DecisionKind::DeclareAttackers | DecisionKind::DeclareBlockers => Choice::Pass,
            _ => d.options[0].choice,
        };
        g.choose(c);
    }
    panic!("quiet_until: no progress");
}

fn player(p: PlayerId) -> Choice {
    Choice::Target(Target::Player(p))
}

#[test]
fn pyrogoyf_the_entering_lhurgoyf_deals_damage_equal_to_its_power() {
    let mut g = scen()
        .battlefield(0, &["Mountain"; 4])
        .hand(0, &["Pyrogoyf"])
        .graveyard(1, &["Island", "Lightning Bolt", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Pyrogoyf");
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(player(1))).then(|| player(1)));
    let goyf = bf(&g, 0, "Pyrogoyf");
    assert_eq!(g.pt(goyf), (3, 4), "land, instant, creature");
    assert_eq!(g.life(1), 17);
}

#[test]
fn screaming_nemesis_redirects_the_damage_and_the_player_cant_gain_life() {
    let mut g = scen()
        .battlefield(0, &["Screaming Nemesis"])
        .battlefield(1, &["Mountain", "Plains", "Test Bear"])
        .hand(1, &["Lightning Bolt", "Swords to Plowshares"])
        .at(Step::Main1, 1)
        .start();
    let nem = bf(&g, 0, "Screaming Nemesis");
    let bear = bf(&g, 1, "Test Bear");
    let bolt = g.card(1, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(g.tgt(nem));
    g.drive_until(at_priority);
    pay(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(player(1))).then(|| player(1)));
    assert_eq!(g.obj(nem).zone, Zone::Graveyard);
    assert_eq!(g.life(1), 17, "3 damage from the Nemesis (last known information)");
    assert!(g.player(1).cant_gain_life);
    // Swords on their own creature: no life gained.
    let stp = g.card(1, "Swords to Plowshares", Zone::Hand);
    g.choose(Choice::Cast(stp));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.life(1), 17);
}

#[test]
fn cr_508_4_adelines_token_enters_tapped_and_attacking() {
    let mut g = scen().battlefield(0, &["Adeline, Resplendent Cathar"]).at(Step::DeclareAttackers, 0).start();
    let adeline = bf(&g, 0, "Adeline, Resplendent Cathar");
    g.choose(Choice::Card(adeline));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    let human = bf(&g, 0, "Human");
    assert!(g.obj(human).attacking && g.obj(human).tapped);
    assert_eq!(g.attackers().len(), 2);
    assert_eq!(g.pt(adeline), (2, 4), "power: the creatures you control");
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 17);
}

#[test]
fn gut_may_sacrifice_another_creature_or_an_artifact_for_an_attacking_skeleton() {
    let mut g = scen().battlefield(0, &["Gut, True Soul Zealot", "Sol Ring", "Island"]).at(Step::DeclareAttackers, 0).start();
    let gut = bf(&g, 0, "Gut, True Soul Zealot");
    let ring = bf(&g, 0, "Sol Ring");
    g.choose(Choice::Card(gut));
    g.choose(Choice::Pass);
    g.drive_until(|g| !at_priority(g) || g.stack().is_empty());
    let mut asked = false;
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::Sacrifice) {
            assert!(g.has_option(Choice::Pass), "you may");
            assert!(!g.has_option(Choice::Card(bf(g, 0, "Island"))), "a creature or an artifact");
            Some(Choice::Card(bf(g, 0, "Sol Ring")))
        } else {
            None
        }
    });
    asked |= g.obj(ring).zone == Zone::Graveyard;
    assert!(asked);
    let sk = bf(&g, 0, "Skeleton");
    assert!(g.obj(sk).attacking);
    assert!(g.has_kw(sk, Kw::Menace));
    quiet_until(&mut g, |g| g.turn().step == Step::Main2);
    assert_eq!(g.life(1), 14);
}

#[test]
fn scythecat_cub_doubles_on_the_second_resolution_this_turn() {
    let mut g = base()
        .library(0, &["Forest", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Scythecat Cub"])
        .hand(0, &["Windswept Heath"])
        .at(Step::Main1, 0)
        .start();
    let cub = bf(&g, 0, "Scythecat Cub");
    let heath = g.card(0, "Windswept Heath", Zone::Hand);
    g.choose(Choice::Play(heath));
    resolve_with(&mut g, |_| None);
    assert_eq!(g.counters_on(cub, Counter::P1P1), 1);
    g.choose(Choice::Ability(heath, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::SearchLibrary) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass),
        _ => None,
    });
    assert_eq!(g.count_in(0, "Forest", Zone::Battlefield), 1, "{}", g.log_lines().join("\n"));
    assert_eq!(g.counters_on(cub, Counter::P1P1), 2, "doubled (1 -> 2)");
    assert_eq!(g.pt(cub), (4, 4));
}

#[test]
fn torsten_puts_the_rest_on_the_bottom_in_a_random_order_nobody_knows() {
    let lib = ["Island", "Test Bear", "Lightning Bolt", "Sol Ring", "Forest", "Counterspell", "Test Flyer", "Plains", "Plains"];
    let mut g = base()
        .library(0, &lib)
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest", "Plains", "Plains", "Plains"])
        .hand(0, &["Torsten, Founder of Benalia"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Torsten, Founder of Benalia");
    // Take every creature and land offered.
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInHand) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| *c != Choice::Pass),
        _ => None,
    });
    let pl = g.player(0);
    let hand: Vec<String> = pl.hand.iter().map(|&c| g.name(c).to_string()).collect();
    assert_eq!(hand.len(), 4, "{hand:?}"); // Test Bear, Forest, Test Flyer, Plains (top seven)
    let bottom: Vec<CardIdx> = pl.library[..3].to_vec();
    let mut names: Vec<&str> = bottom.iter().map(|&c| g.name(c)).collect();
    names.sort();
    assert_eq!(names, ["Counterspell", "Lightning Bolt", "Sol Ring"]);
    for &c in &bottom {
        assert!(!g.knows(0, c) && !g.knows(1, c), "random order: nobody knows which is where");
    }
    assert!(
        g.clouds().iter().any(|k| k.viewer == 1 && k.owner == 0 && k.library && k.defs.len() == 3),
        "the opponent saw them go into that library"
    );
}

#[test]
fn endurance_up_to_one_target_player_shuffles_their_graveyard_to_the_bottom() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest"])
        .hand(0, &["Endurance"])
        .graveyard(1, &["Lightning Bolt", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let endurance = g.card(0, "Endurance", Zone::Hand);
    g.choose(Choice::Cast(endurance));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::ChooseAltCost));
    if g.kind() == Some(DecisionKind::ChooseAltCost) {
        g.choose(Choice::Number(0));
    }
    g.drive_until(at_priority);
    pay(&mut g);
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Target) && g.has_option(player(1))).then(|| player(1)));
    assert!(g.player(1).graveyard.is_empty());
    assert_eq!(g.player(1).library.len(), 12);
    let lib = &g.player(1).library;
    let mut names: Vec<&str> = lib[..2].iter().map(|&c| g.name(c)).collect();
    names.sort();
    assert_eq!(names, ["Lightning Bolt", "Test Bear"]);
}

#[test]
fn get_lost_its_controller_creates_two_map_tokens() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains"])
        .hand(0, &["Get Lost"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let gl = g.card(0, "Get Lost", Zone::Hand);
    g.choose(Choice::Cast(gl));
    g.choose(g.tgt(bear));
    g.drive_until(at_priority);
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.obj(bear).zone, Zone::Graveyard);
    assert_eq!(g.count_in(1, "Map", Zone::Battlefield), 2);
    assert_eq!(g.count_in(0, "Map", Zone::Battlefield), 0);
}

#[test]
fn psychic_frog_exiles_three_graveyard_cards_as_a_cost() {
    let mut g = scen()
        .battlefield(0, &["Psychic Frog"])
        .graveyard(0, &["Island", "Lightning Bolt", "Test Bear", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let frog = bf(&g, 0, "Psychic Frog");
    g.choose(Choice::Ability(frog, AbilitySlot::Activated(1)));
    let mut picks = 0;
    while g.kind() == Some(DecisionKind::ChooseGraveyardCard) {
        let opts = g.pending_decision().unwrap().options.len();
        assert!(opts <= 2, "a set in increasing position: {opts} options");
        let c = g.pending_decision().unwrap().options[0].choice;
        g.choose(c);
        picks += 1;
    }
    assert_eq!(picks, 3);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).graveyard.len(), 1);
    assert!(g.has_kw(frog, Kw::Flying));
    let g2 = scen().battlefield(0, &["Psychic Frog"]).graveyard(0, &["Island", "Island"]).at(Step::Main1, 0).start();
    let frog = bf(&g2, 0, "Psychic Frog");
    assert!(!g2.has_option(Choice::Ability(frog, AbilitySlot::Activated(1))), "two cards are not enough");
}

#[test]
fn cr_603_12_leatherhead_removes_a_counter_then_the_reflexive_trigger_targets() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Leatherhead, Swamp Stalker"])
        .battlefield(1, &["Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Leatherhead, Swamp Stalker");
    resolve_with(&mut g, |_| None);
    let lh = bf(&g, 0, "Leatherhead, Swamp Stalker");
    let ring = bf(&g, 1, "Sol Ring");
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().number > t0 && g.turn().active == 0 && g.kind() == Some(DecisionKind::DeclareAttackers));
    g.choose(Choice::Card(lh));
    g.choose(Choice::Pass);
    let mut removed = false;
    for _ in 0..200 {
        g.drive_until(|g| !at_priority(g) || g.turn().step == Step::Main2);
        if g.turn().step == Step::Main2 {
            break;
        }
        match g.kind() {
            Some(DecisionKind::ChooseMode) => {
                assert!(g.has_option(Choice::Pass), "you may");
                removed = true;
                g.choose(Choice::Number(0));
            }
            Some(DecisionKind::Target) => {
                assert!(removed, "targets are chosen after the counter is removed");
                g.choose(g.tgt(ring));
            }
            Some(DecisionKind::DeclareBlockers) => {
                g.choose(Choice::Pass);
            }
            _ => {
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
            }
        }
    }
    assert!(removed);
    assert_eq!(g.life(1), 15);
    assert_eq!(g.obj(ring).zone, Zone::Graveyard);
    assert!(!g.has_kw(lh, Kw::Hexproof), "the hexproof counter is gone");
}

#[test]
fn leatherhead_enters_with_a_hexproof_counter_it_can_remove() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Forest", "Forest", "Forest"])
        .hand(0, &["Leatherhead, Swamp Stalker"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Leatherhead, Swamp Stalker");
    resolve_with(&mut g, |_| None);
    let lh = bf(&g, 0, "Leatherhead, Swamp Stalker");
    assert_eq!(g.counters_on(lh, Counter::Kw(Kw::Hexproof)), 1);
    assert!(g.has_kw(lh, Kw::Hexproof));
}

#[test]
fn bloodtithe_harvester_minus_x_where_x_is_twice_your_blood_tokens() {
    let mut g = scen()
        .battlefield(0, &["Swamp", "Mountain"])
        .hand(0, &["Bloodtithe Harvester"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    cast(&mut g, 0, "Bloodtithe Harvester");
    resolve_with(&mut g, |_| None);
    assert_eq!(g.count_in(0, "Blood", Zone::Battlefield), 1);
    let bh = bf(&g, 0, "Bloodtithe Harvester");
    let giant = bf(&g, 1, "Test Giant");
    // Summoning sick: {T} can't be paid this turn.
    assert!(!g.has_option(Choice::Ability(bh, AbilitySlot::Activated(0))));
    let t0 = g.turn().number;
    quiet_until(&mut g, |g| g.turn().active == 0 && g.turn().step == Step::Main1 && at_priority(g) && g.turn().number > t0);
    g.choose(Choice::Ability(bh, AbilitySlot::Activated(0)));
    g.drive_until(|g| g.kind() == Some(DecisionKind::Target) || at_priority(g));
    if g.kind() == Some(DecisionKind::Target) {
        g.choose(g.tgt(giant));
    }
    resolve_with(&mut g, |_| None);
    assert_eq!(g.pt(giant), (3, 3), "-2/-2: one Blood token");
}

#[test]
fn sewer_veillance_cam_may_tap_or_untap_target_creature() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .hand(0, &["Sewer-veillance Cam"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    cast(&mut g, 0, "Sewer-veillance Cam");
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        Some(DecisionKind::ChooseMode) => Some(Choice::Number(0)),
        _ => None,
    });
    assert!(g.obj(bear).tapped);
}

#[test]
fn formidable_speaker_untaps_another_target_permanent() {
    let mut g = scen()
        .battlefield(0, &["Formidable Speaker", "Forest", "Island"])
        .at(Step::Main1, 0)
        .start();
    let sp = bf(&g, 0, "Formidable Speaker");
    let island = bf(&g, 0, "Island");
    g.choose(Choice::Ability(sp, AbilitySlot::Activated(0)));
    for _ in 0..20 {
        match g.kind() {
            Some(DecisionKind::Target) => {
                assert!(!g.has_option(g.tgt(sp)), "another");
                g.choose(g.tgt(island));
            }
            Some(DecisionKind::PayMana) => {
                // Pay {1} with the Island: then untap it.
                let c = g
                    .pending_decision()
                    .unwrap()
                    .options
                    .iter()
                    .map(|o| o.choice)
                    .find(|c| g.describe_choice(*c).contains("Island"))
                    .unwrap_or(g.pending_decision().unwrap().options[0].choice);
                g.choose(c);
            }
            _ => break,
        }
        g.drive_until(|g| !at_priority(g) || !g.stack().is_empty());
    }
    resolve_with(&mut g, |_| None);
    assert!(g.obj(sp).tapped);
    assert!(!g.obj(island).tapped || g.obj(bf(&g, 0, "Forest")).tapped);
}
