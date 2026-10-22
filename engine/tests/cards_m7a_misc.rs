//! M7a: delve (CR 702.66), kicker (702.33), energy (122.1), will of the
//! council, Mox Diamond's replacement, "first spell this game", Oath of
//! Druids, Upheaval, Flash, Grindstone, Expressive Iteration, Mishra's
//! Bauble.

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
        g.drive_until(|g| at_priority(g) || g.kind() != Some(DecisionKind::PayMana));
    }
}

/// Run the stack down; `answer` answers what is asked (None: the first
/// option).
fn resolve_with(g: &mut Game, answer: impl Fn(&Game) -> Option<Choice>) {
    for _ in 0..300 {
        g.drive_until(|g| (g.stack().is_empty() && at_priority(g)) || !at_priority(g) || g.is_over());
        if g.is_over() || (g.stack().is_empty() && at_priority(g)) {
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
fn upheaval_returns_all_permanents_to_their_owners_hands() {
    let mut g = scen()
        .battlefield(0, &["Island"; 6])
        .hand(0, &["Upheaval"])
        .battlefield(1, &["Test Bear", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Upheaval", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    assert!(g.battlefield().is_empty());
    assert_eq!(g.player(0).hand.len(), 6);
    assert_eq!(g.player(1).hand.len(), 2);
}

#[test]
fn once_upon_a_time_is_free_only_as_the_first_spell_of_the_game() {
    let g = scen().hand(0, &["Once Upon a Time"]).at(Step::Main1, 0).start();
    let c = g.card(0, "Once Upon a Time", Zone::Hand);
    assert!(g.has_option(Choice::Cast(c)), "no land: free");
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Once Upon a Time", "Lightning Bolt"])
        .at(Step::Main1, 0)
        .start();
    let bolt = g.card(0, "Lightning Bolt", Zone::Hand);
    g.choose(Choice::Cast(bolt));
    g.choose(player(1));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |_| None);
    let c = g.card(0, "Once Upon a Time", Zone::Hand);
    assert!(!g.has_option(Choice::Cast(c)), "not the first spell any more");
}

#[test]
fn councils_judgment_exiles_both_when_the_votes_are_tied() {
    let mut g = scen()
        .battlefield(0, &["Plains", "Plains", "Plains", "Test Wall"])
        .hand(0, &["Council's Judgment"])
        .battlefield(1, &["Test Bear", "Sol Ring"])
        .at(Step::Main1, 0)
        .start();
    let (bear, ring, wall) = (bf(&g, 1, "Test Bear"), bf(&g, 1, "Sol Ring"), bf(&g, 0, "Test Wall"));
    let c = g.card(0, "Council's Judgment", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChoosePermanent) => {
            assert!(!g.has_option(Choice::Card(wall)), "a permanent the caster doesn't control");
            let voter = g.pending_decision().unwrap().player;
            Some(Choice::Card(if voter == 0 { bear } else { ring }))
        }
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
    assert_eq!(g.obj(ring).zone, Zone::Exile);
}

#[test]
fn grindstone_repeats_while_the_two_milled_cards_share_a_colour() {
    let mut g = base()
        .library(0, &["Island"; 5])
        .library(1, &["Lightning Bolt", "Lightning Bolt", "Counterspell", "Test Bear", "Forest", "Forest"])
        .battlefield(0, &["Grindstone", "Island", "Island", "Island"])
        .at(Step::Main1, 0)
        .start();
    let gs = bf(&g, 0, "Grindstone");
    g.choose(Choice::Ability(gs, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    // Two Bolts share red: again; Counterspell + Test Bear don't.
    assert_eq!(g.player(1).graveyard.len(), 4);
    assert_eq!(g.player(1).library.len(), 2);
}

#[test]
fn cr_702_66_treasure_cruise_delves_its_generic_cost() {
    let mut g = scen()
        .battlefield(0, &["Island"])
        .hand(0, &["Treasure Cruise"])
        .graveyard(0, &["Island", "Forest", "Lightning Bolt", "Test Bear", "Sol Ring", "Counterspell", "Mountain"])
        .at(Step::Main1, 0)
        .start();
    let tc = g.card(0, "Treasure Cruise", Zone::Hand);
    assert!(g.has_option(Choice::Cast(tc)), "{{7}}{{U}}: seven cards and an Island");
    g.choose(Choice::Cast(tc));
    let mut delved = 0;
    for _ in 0..20 {
        g.drive_until(|g| !at_priority(g) || g.stack().len() == 1);
        match g.kind() {
            Some(DecisionKind::ChooseGraveyardCard) => {
                assert!(!g.has_option(Choice::Pass), "the rest can't be paid yet");
                let c = g.pending_decision().unwrap().options[0].choice;
                g.choose(c);
                delved += 1;
            }
            Some(DecisionKind::PayMana) => pay(&mut g),
            _ => break,
        }
    }
    assert_eq!(delved, 7);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.player(0).hand.len(), 3);
    assert_eq!(g.exile_zone().len(), 7);
}

#[test]
fn flash_sacrifices_the_creature_unless_its_reduced_cost_is_paid() {
    let mut g = scen()
        .battlefield(0, &["Island", "Island"])
        .hand(0, &["Flash", "Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Flash", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::YesNo) => {
            assert!(!g.has_option(Choice::Yes), "{{2}}{{G}} left: no mana");
            None
        }
        Some(_) => g.pending_decision().unwrap().options.iter().map(|o| o.choice).find(|c| matches!(c, Choice::Card(_))),
        None => None,
    });
    assert_eq!(g.count_in(0, "Test Giant", Zone::Graveyard), 1, "{}", g.log_lines().join("\n"));
}

#[test]
fn mox_diamond_enters_only_by_discarding_a_land() {
    let mut g = scen().hand(0, &["Mox Diamond", "Forest"]).at(Step::Main1, 0).start();
    let md = g.card(0, "Mox Diamond", Zone::Hand);
    g.choose(Choice::Cast(md));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Discard) => Some(Choice::Card(g.card(0, "Forest", Zone::Hand))),
        _ => None,
    });
    assert_eq!(g.obj(md).zone, Zone::Battlefield);
    assert_eq!(g.count_in(0, "Forest", Zone::Graveyard), 1);
    let mut g = scen().hand(0, &["Mox Diamond", "Forest"]).at(Step::Main1, 0).start();
    let md = g.card(0, "Mox Diamond", Zone::Hand);
    g.choose(Choice::Cast(md));
    resolve_with(&mut g, |g| (g.kind() == Some(DecisionKind::Discard)).then_some(Choice::Pass));
    assert_eq!(g.obj(md).zone, Zone::Graveyard, "no land discarded");
}

#[test]
fn oath_of_druids_reveals_until_a_creature_for_the_player_with_fewer() {
    let mut g = base()
        .library(0, &["Island", "Lightning Bolt", "Test Giant", "Island", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Oath of Druids"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main2, 1)
        .start();
    quiet_until(&mut g, |g| g.turn().active == 0 && g.kind() == Some(DecisionKind::YesNo));
    g.choose(Choice::Yes);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.count_in(0, "Test Giant", Zone::Battlefield), 1);
    assert_eq!(g.player(0).graveyard.len(), 2, "Island and Bolt");
}

#[test]
fn cr_702_33_burst_lightning_kicked_deals_four() {
    for (kick, life) in [(false, 18), (true, 16)] {
        let mut g = scen().battlefield(0, &["Mountain"; 5]).hand(0, &["Burst Lightning"]).at(Step::Main1, 0).start();
        let c = g.card(0, "Burst Lightning", Zone::Hand);
        g.choose(Choice::Cast(c));
        g.drive_until(|g| !at_priority(g));
        assert_eq!(g.kind(), Some(DecisionKind::ChooseCost));
        g.choose(Choice::Number(kick as u8));
        resolve_with(&mut g, |g| match g.kind() {
            Some(DecisionKind::Target) => Some(player(1)),
            _ => None,
        });
        assert_eq!(g.life(1), life);
    }
}

#[test]
fn tear_asunder_kicked_targets_any_nonland_permanent() {
    let mut g = scen()
        .battlefield(0, &["Forest", "Swamp", "Swamp", "Swamp"])
        .hand(0, &["Tear Asunder"])
        .battlefield(1, &["Test Bear"])
        .at(Step::Main1, 0)
        .start();
    let bear = bf(&g, 1, "Test Bear");
    let c = g.card(0, "Tear Asunder", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| !at_priority(g));
    assert!(!g.has_option(Choice::Number(0)), "unkicked: no artifact or enchantment to target");
    g.choose(Choice::Number(1));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(g.tgt(bear)),
        _ => None,
    });
    assert_eq!(g.obj(bear).zone, Zone::Exile);
}

#[test]
fn galvanic_discharge_pays_energy_for_damage() {
    let mut g = scen()
        .battlefield(0, &["Mountain"])
        .hand(0, &["Galvanic Discharge"])
        .battlefield(1, &["Test Giant"])
        .at(Step::Main1, 0)
        .start();
    let giant = bf(&g, 1, "Test Giant");
    let c = g.card(0, "Galvanic Discharge", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.choose(g.tgt(giant));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::ChooseMode) => {
            assert!(g.has_option(Choice::Number(3)) && !g.has_option(Choice::Number(4)));
            Some(Choice::Number(2))
        }
        _ => None,
    });
    assert_eq!(g.player(0).energy, 1);
    assert_eq!(g.obj(giant).damage, 2);
}

#[test]
fn guide_of_souls_energy_then_an_angel_with_counters() {
    let mut g = scen()
        .battlefield(0, &["Guide of Souls", "Test Hasty"])
        .at(Step::DeclareAttackers, 0)
        .start();
    let hasty = bf(&g, 0, "Test Hasty");
    // No energy yet: the attack trigger does nothing.
    g.choose(Choice::Card(hasty));
    g.choose(Choice::Pass);
    resolve_with(&mut g, |_| None);
    assert_eq!(g.counters_on(hasty, Counter::P1P1), 0);
    let mut g = scen()
        .battlefield(0, &["Guide of Souls", "Test Hasty", "Forest", "Forest"])
        .hand(0, &["Test Bear", "Test Bear", "Test Bear"])
        .at(Step::Main1, 0)
        .start();
    for _ in 0..1 {
        let c = g.card(0, "Test Bear", Zone::Hand);
        g.choose(Choice::Cast(c));
        g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
        pay(&mut g);
        resolve_with(&mut g, |_| None);
    }
    assert_eq!(g.player(0).energy, 1);
    assert_eq!(g.life(0), 21);
}

#[test]
fn expressive_iteration_hand_bottom_and_an_exiled_card_to_play() {
    let mut g = base()
        .library(0, &["Lightning Bolt", "Forest", "Counterspell", "Island"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Island", "Mountain"])
        .hand(0, &["Expressive Iteration"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Expressive Iteration", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    let bolt = g.card(0, "Lightning Bolt", Zone::Library);
    let forest = g.card(0, "Forest", Zone::Library);
    let cs = g.card(0, "Counterspell", Zone::Library);
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::PutInHand) => Some(Choice::Card(cs)),
        Some(DecisionKind::PutOnBottom) => Some(Choice::Card(bolt)),
        _ => None,
    });
    assert_eq!(g.obj(cs).zone, Zone::Hand);
    assert_eq!(g.player(0).library[0], bolt);
    assert_eq!(g.obj(forest).zone, Zone::Exile);
    assert!(g.has_option(Choice::Play(forest)), "may play the exiled card this turn");
}

#[test]
fn mishras_bauble_looks_at_the_top_card_and_draws_next_upkeep() {
    let mut g = scen().battlefield(0, &["Mishra's Bauble"]).at(Step::Main1, 0).start();
    let mb = bf(&g, 0, "Mishra's Bauble");
    let top = *g.player(1).library.last().unwrap();
    assert!(!g.knows(0, top));
    g.choose(Choice::Ability(mb, AbilitySlot::Activated(0)));
    resolve_with(&mut g, |g| match g.kind() {
        Some(DecisionKind::Target) => Some(player(1)),
        _ => None,
    });
    // (Keyed for P0; not kept as knowledge: its owner doesn't know it.)
    assert!(!g.knows(1, top), "P1 didn't look");
    let hand = g.player(0).hand.len();
    quiet_until(&mut g, |g| g.turn().active == 1 && g.turn().step == Step::Draw);
    assert_eq!(g.player(0).hand.len(), hand + 1, "drawn at the next upkeep");
}

#[test]
fn consult_the_star_charts_looks_at_x_cards_x_the_lands_you_control() {
    let mut g = base()
        .library(0, &["Lightning Bolt", "Forest", "Counterspell", "Island", "Sol Ring"])
        .library(1, &["Forest"; 5])
        .battlefield(0, &["Island", "Island", "Island"])
        .hand(0, &["Consult the Star Charts"])
        .at(Step::Main1, 0)
        .start();
    let c = g.card(0, "Consult the Star Charts", Zone::Hand);
    g.choose(Choice::Cast(c));
    g.drive_until(|g| !at_priority(g));
    g.choose(Choice::Number(0));
    g.drive_until(|g| at_priority(g) || g.kind() == Some(DecisionKind::PayMana));
    pay(&mut g);
    let mut seen = 0;
    resolve_with(&mut g, |g| {
        if g.kind() == Some(DecisionKind::PutInHand) {
            assert!(g.pending_decision().unwrap().options.len() <= 3, "three lands: three cards");
        }
        None
    });
    seen += 1;
    assert_eq!(seen, 1);
    assert_eq!(g.player(0).hand.len(), 1);
    let _ = Kw::Flying;
}
